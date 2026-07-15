//! The core agent loop: prompt → LLM stream → tool execution → repeat.
//!
//! - `agent_loop()` starts with new prompt messages
//! - `agent_loop_continue()` resumes from existing context
//!
//! Both return a stream of `AgentEvent`s.

use tokio::sync::mpsc;

use super::assistant_sanitize::sanitize_assistant_text;
use super::compaction::post_response_compaction;
use super::compaction::pre_prompt_compaction;
use super::config::AgentLoopConfig;
use super::doom_loop::DoomLoopDetector;
use super::input_filter::apply_input_filters;
use super::llm_call::stream_assistant_response;
use super::thinking_only_guard::ThinkingOnlyGuard;
use super::tool_exec::execute_tool_calls;
use super::tool_exec::skip_tool_call_doom_loop;
use crate::context::ContextTracker;
use crate::context::ExecutionTracker;
use crate::context::{self};
use crate::provider::ToolDefinition;
use crate::types::*;

/// Start an agent loop with new prompt messages.
pub async fn agent_loop(
    prompts: Vec<AgentMessage>,
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Vec<AgentMessage> {
    tx.send(AgentEvent::AgentStart).ok();

    // Apply input filters
    let prompts = match apply_input_filters(prompts, &config.input_filters, &tx) {
        Some(p) => p,
        None => return vec![],
    };

    let mut new_messages: Vec<AgentMessage> = prompts.clone();

    // Add prompts to context
    for prompt in &prompts {
        context.messages.push(prompt.clone());
    }

    tx.send(AgentEvent::TurnStart).ok();

    // Emit events for each prompt message
    for prompt in &prompts {
        tx.send(AgentEvent::MessageStart {
            message: prompt.clone(),
        })
        .ok();
        tx.send(AgentEvent::MessageEnd {
            message: prompt.clone(),
        })
        .ok();
    }

    run_loop(context, &mut new_messages, config, &tx, &cancel).await;

    tx.send(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .ok();
    new_messages
}

/// Continue an agent loop from existing context (for retries).
pub async fn agent_loop_continue(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> Vec<AgentMessage> {
    tx.send(AgentEvent::AgentStart).ok();

    if context.messages.is_empty() {
        tx.send(AgentEvent::Error {
            error: AgentErrorInfo {
                kind: AgentErrorKind::Runtime,
                message: "Cannot continue: no messages in context".into(),
            },
        })
        .ok();
        tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
        return vec![];
    }

    if let Some(last) = context.messages.last() {
        if last.role() == "assistant" {
            tx.send(AgentEvent::Error {
                error: AgentErrorInfo {
                    kind: AgentErrorKind::Runtime,
                    message: "Cannot continue from assistant message".into(),
                },
            })
            .ok();
            tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
            return vec![];
        }
    }

    let mut new_messages: Vec<AgentMessage> = Vec::new();

    tx.send(AgentEvent::TurnStart).ok();

    run_loop(context, &mut new_messages, config, &tx, &cancel).await;

    tx.send(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    })
    .ok();
    new_messages
}

/// Main loop logic shared by agent_loop and agent_loop_continue.
///
/// Outer loop: continues when follow-up messages arrive after agent would stop.
/// Inner loop: process tool calls and steering messages.
async fn run_loop(
    context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
) {
    let mut first_turn = true;
    let mut turn_number: usize = 0;
    let mut tracker = config
        .execution_limits
        .as_ref()
        .map(|limits| ExecutionTracker::new(limits.clone()));
    let mut doom_detector = DoomLoopDetector::new(3);

    let mut thinking_only_guard = ThinkingOnlyGuard::new();
    let mut context_tracker = ContextTracker::new();
    let mut compaction_controller = config.context_config.as_ref().map(|ctx_cfg| {
        crate::context::CompactionController::new(
            crate::context::CompactionConfig::from_context_config(ctx_cfg),
        )
    });

    // Check for steering messages at start
    let mut pending: Vec<AgentMessage> = config
        .get_steering_messages
        .as_ref()
        .map(|f| f())
        .unwrap_or_default();

    // Outer loop: follow-ups after agent would stop
    loop {
        if cancel.is_cancelled() {
            return;
        }

        let mut steering_after_tools: Option<Vec<AgentMessage>> = None;

        // Inner loop: runs at least once, then continues if tool calls or pending messages
        loop {
            if cancel.is_cancelled() {
                return;
            }

            if !first_turn {
                tx.send(AgentEvent::TurnStart).ok();
            } else {
                first_turn = false;
            }

            // Inject pending messages (steering / follow-up / initial prompt)
            let injected_count = pending.len();
            if !pending.is_empty() {
                for msg in pending.drain(..) {
                    tx.send(AgentEvent::MessageStart {
                        message: msg.clone(),
                    })
                    .ok();
                    tx.send(AgentEvent::MessageEnd {
                        message: msg.clone(),
                    })
                    .ok();
                    context.messages.push(msg.clone());
                    new_messages.push(msg);
                }
            }

            // Check execution limits
            if let Some(ref tracker) = tracker {
                if let Some(reason) = tracker.check_limits() {
                    let limit_msg = AgentMessage::Llm(Message::User {
                        content: vec![Content::Text {
                            text: format!("[Agent stopped: {}]", reason),
                        }],
                        timestamp: now_ms(),
                    });
                    tx.send(AgentEvent::MessageStart {
                        message: limit_msg.clone(),
                    })
                    .ok();
                    tx.send(AgentEvent::MessageEnd {
                        message: limit_msg.clone(),
                    })
                    .ok();
                    context.messages.push(limit_msg.clone());
                    new_messages.push(limit_msg);
                    return;
                }
            }

            // before_turn callback — abort if it returns false
            if let Some(ref before_turn) = config.before_turn {
                if !before_turn(&context.messages, turn_number) {
                    return;
                }
            }
            turn_number += 1;

            let tool_defs: Vec<ToolDefinition> = context
                .tools
                .iter()
                .map(|t| ToolDefinition {
                    name: t.resolve_name(&config.model),
                    description: crate::tools::resolve_tool_refs(
                        t.description(),
                        &context.tools,
                        &config.model,
                    ),
                    parameters: t.parameters_schema(),
                })
                .collect();
            context_tracker.record_request_overhead(&context.system_prompt, &tool_defs);

            // Every provider call gets a preflight check, not just the first
            // call of a run. Tool results and injected steering can grow a long
            // agent run past the threshold between turns.
            if !pre_prompt_compaction(
                &mut compaction_controller,
                &mut context_tracker,
                &mut context.messages,
                config,
                cancel.clone(),
                tx,
            )
            .await
            {
                return;
            }

            // Build budget snapshot for the LLM call (same source as compaction)
            let budget_snapshot =
                context_tracker.budget_snapshot(&context.messages, config.context_config.as_ref());

            // Stream assistant response
            let assistant_result = stream_assistant_response(
                context,
                config,
                tx,
                cancel,
                turn_number,
                injected_count,
                budget_snapshot,
            )
            .await;
            let message = assistant_result.message;

            // Strip any `<system-reminder>` / `<system>` tags or status-template
            // preambles the model may have mimicked from reminders it saw in
            // context. Without this, the fake tags land back in the prompt next
            // turn and teach the model to keep producing them.
            let message = sanitize_message(message);

            let agent_msg: AgentMessage = message.clone().into();
            context.messages.push(agent_msg.clone());
            new_messages.push(agent_msg.clone());

            // Clear the post-compaction stale flag once a real response lands,
            // re-enabling the provider anchor (which is read from the message
            // list itself, so no token count needs to be stored here).
            if let Message::Assistant { ref usage, .. } = message {
                context_tracker.record_response(usage);
            }

            // Extract tool calls before compaction. A tool-use assistant message
            // must stay adjacent to its tool results; compacting before results
            // are appended creates orphaned pairs that provider APIs reject.
            let tool_calls: Vec<_> = match &message {
                Message::Assistant { content, .. } => content
                    .iter()
                    .filter_map(|c| match c {
                        Content::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some((id.clone(), name.clone(), arguments.clone())),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            };

            let has_tool_calls = !tool_calls.is_empty();

            // Post-response compaction check (overflow recovery + threshold).
            // For tool-use responses, defer threshold compaction until after
            // tool results have been appended so the message list is valid.
            if !has_tool_calls {
                let should_retry = post_response_compaction(
                    &mut compaction_controller,
                    &mut context_tracker,
                    &mut context.messages,
                    &message,
                    config,
                    cancel.clone(),
                    tx,
                )
                .await;
                if should_retry {
                    // Remove the agent_msg we just pushed to new_messages.
                    // The controller already removed it from context.messages.
                    new_messages.pop();
                    continue;
                }
            }

            // Tool-use responses defer compaction until after tool results, but
            // the assistant message itself is accepted before tool execution.
            if has_tool_calls {
                tx.send(AgentEvent::MessageEnd {
                    message: agent_msg.clone(),
                })
                .ok();
                if let Some(ctrl) = compaction_controller.as_mut() {
                    ctrl.on_success();
                }
            }

            // Check for error/abort
            if let Message::Assistant {
                ref stop_reason,
                ref error_message,
                ref usage,
                ..
            } = message
            {
                if *stop_reason == StopReason::Error || *stop_reason == StopReason::Aborted {
                    // Emit unified Error event for provider errors (but not cancellations)
                    if *stop_reason == StopReason::Error && !cancel.is_cancelled() {
                        let err_str = error_message
                            .as_deref()
                            .unwrap_or("Unknown error")
                            .to_string();
                        tx.send(AgentEvent::Error {
                            error: AgentErrorInfo {
                                kind: AgentErrorKind::Provider,
                                message: err_str,
                            },
                        })
                        .ok();
                    }
                    // Call after_turn even on error/abort so callers tracking usage don't miss this turn
                    if let Some(ref after_turn) = config.after_turn {
                        after_turn(&context.messages, usage);
                    }
                    tx.send(AgentEvent::TurnEnd {
                        message: agent_msg,
                        tool_results: vec![],
                    })
                    .ok();
                    return;
                }
            }

            // Doom-loop detection: if the same tool batch repeats >= threshold
            // times, skip execution and inject a steering message instead.
            if has_tool_calls {
                if let Some(intervention) = doom_detector.check(&tool_calls) {
                    let mut tool_results = Vec::new();
                    for (id, name, args) in &tool_calls {
                        let result = skip_tool_call_doom_loop(id, name, args, tx);
                        let am: AgentMessage = result.clone().into();
                        context.messages.push(am.clone());
                        new_messages.push(am);
                        tool_results.push(result);
                    }
                    pending.push(intervention.steering_message);

                    // Track turn + emit TurnEnd, then continue inner loop.
                    if let Some(ref mut tracker) = tracker {
                        let turn_tokens = match &message {
                            Message::Assistant { usage, .. } => usage.context_tokens() as usize,
                            _ => context::message_tokens(&agent_msg),
                        };
                        tracker.record_turn(turn_tokens);
                    }
                    if let Some(ref after_turn) = config.after_turn {
                        let usage = match &message {
                            Message::Assistant { usage, .. } => usage.clone(),
                            _ => Usage::default(),
                        };
                        after_turn(&context.messages, &usage);
                    }
                    tx.send(AgentEvent::TurnEnd {
                        message: agent_msg,
                        tool_results,
                    })
                    .ok();
                    continue;
                }
            }

            let mut tool_results = Vec::new();
            if has_tool_calls {
                let idle_clock = tracker.as_ref().map(|t| t.idle_clock());
                let execution = execute_tool_calls(
                    &context.tools,
                    &tool_calls,
                    tx,
                    cancel,
                    config.get_steering_messages.as_ref(),
                    &config.tool_execution,
                    &context.cwd,
                    &context.path_guard,
                    &config.spill,
                    idle_clock.as_ref(),
                    config
                        .model_config
                        .as_ref()
                        .map(|m| m.supports_image())
                        .unwrap_or(true),
                )
                .await;

                tool_results = execution.tool_results;
                steering_after_tools = execution.steering_messages;

                for result in &tool_results {
                    let am: AgentMessage = result.clone().into();
                    context.messages.push(am.clone());
                    new_messages.push(am);
                }

                if steering_after_tools.is_none() {
                    let steering = config
                        .get_steering_messages
                        .as_ref()
                        .map(|f| f())
                        .unwrap_or_default();
                    if !steering.is_empty() {
                        steering_after_tools = Some(steering);
                    }
                }
            }

            // Track turn for execution limits
            if let Some(ref mut tracker) = tracker {
                let turn_tokens = match &message {
                    Message::Assistant { usage, .. } => usage.context_tokens() as usize,
                    _ => context::message_tokens(&agent_msg),
                };
                tracker.record_turn(turn_tokens);
            }

            // after_turn callback
            if let Some(ref after_turn) = config.after_turn {
                let usage = match &message {
                    Message::Assistant { usage, .. } => usage.clone(),
                    _ => Usage::default(),
                };
                after_turn(&context.messages, &usage);
            }

            tx.send(AgentEvent::TurnEnd {
                message: agent_msg,
                tool_results,
            })
            .ok();

            // Check steering after turn
            if let Some(steering) = steering_after_tools.take() {
                if !steering.is_empty() {
                    pending = steering;
                    continue;
                }
            }

            if !pending.is_empty() {
                continue;
            }

            pending = config
                .get_steering_messages
                .as_ref()
                .map(|f| f())
                .unwrap_or_default();

            // Exit inner loop if no more tool calls and no pending messages
            if !has_tool_calls && pending.is_empty() {
                // Check for thinking-only response (model produced thinking but
                // no visible text or tool calls). Nudge it to produce output.
                if let Some(nudge) = thinking_only_guard.check(&message, has_tool_calls) {
                    pending = vec![nudge];
                    continue;
                }
                break;
            }
        }

        // Agent would stop. Check for follow-ups.
        let follow_ups = config
            .get_follow_up_messages
            .as_ref()
            .map(|f| f())
            .unwrap_or_default();

        if !follow_ups.is_empty() {
            pending = follow_ups;
            continue;
        }

        break;
    }
}

/// Run the assistant-text sanitizer over every `Content::Text` block in an
/// Assistant message. Non-assistant variants pass through unchanged.
fn sanitize_message(message: Message) -> Message {
    match message {
        Message::Assistant {
            mut content,
            stop_reason,
            model,
            provider,
            usage,
            timestamp,
            error_message,
            response_id,
        } => {
            for block in content.iter_mut() {
                if let Content::Text { text } = block {
                    let cleaned = sanitize_assistant_text(text);
                    if cleaned != *text {
                        *text = cleaned;
                    }
                }
            }
            // Drop text blocks that sanitized to empty — keeping them would
            // serialize as zero-length assistant text, which some providers
            // reject on the next turn.
            content.retain(|c| !matches!(c, Content::Text { text } if text.trim().is_empty()));
            Message::Assistant {
                content,
                stop_reason,
                model,
                provider,
                usage,
                timestamp,
                error_message,
                response_id,
            }
        }
        other => other,
    }
}
