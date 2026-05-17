//! Engine runtime — create engine, forward events, orchestrate a run.
//!
//! This module owns the boundary between `evot_engine::AgentEvent` and the
//! app-layer `RunEvent`. No engine types leak beyond this module.
//!
//! A single `Run` may comprise multiple internal engine turns when the
//! session has an active goal: `execute_run` drives an outer loop that
//! consults `goal::policy` between turns and rebuilds the engine via the
//! `TurnFactory`. Consumers see one `RunStarted` and one aggregated
//! `RunFinished`.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::control::RunControl;
use super::convert::assistant_blocks_from_content;
use super::convert::extract_content_text;
use super::convert::from_agent_messages;
use super::convert::total_usage;
use super::convert::transcript_from_assistant_completed;
use super::event::LlmMessageStats;
use super::event::LlmToolCallSummary;
use super::event::RunEvent;
use super::event::RunEventContext;
use super::event::RunEventPayload;
use super::run::Run;
use crate::agent::goal::runtime::AfterTurn;
use crate::agent::goal::runtime::GoalTurnReport;
use crate::agent::session::Session;
use crate::conf::Protocol;
use crate::error::Result;

pub type RunFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Run>> + Send + 'a>>;

/// Closure type for goal evaluation: sends a prompt to the model, returns raw text.
pub(in crate::agent) type EvalFn =
    Arc<dyn Fn(String) -> futures::future::BoxFuture<'static, Result<String>> + Send + Sync>;
use crate::types::CompactRecord;
use crate::types::ContextCompactionCompletedStats;
use crate::types::ContextCompactionStartedStats;
use crate::types::LlmCallCompletedStats;
use crate::types::LlmCallMetrics;
use crate::types::LlmCallRetryStats;
use crate::types::LlmCallStartedStats;
use crate::types::RunFinishedStats;
use crate::types::ToolFinishedStats;
use crate::types::TranscriptItem;
use crate::types::TranscriptStats;
use crate::types::UsageSummary;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

pub struct EngineOptions {
    pub provider: String,
    pub protocol: Protocol,
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub limits: crate::agent::ExecutionLimits,
    pub skills_dirs: Vec<std::path::PathBuf>,
    pub tools: Vec<Box<dyn evot_engine::AgentTool>>,
    pub thinking_level: evot_engine::ThinkingLevel,
    pub compat_caps: evot_engine::provider::CompatCaps,
    pub cwd: std::path::PathBuf,
    pub path_guard: std::sync::Arc<evot_engine::PathGuard>,
    pub spill_dir: Option<std::path::PathBuf>,
    pub prompt_cache_key: Option<String>,
}

// ---------------------------------------------------------------------------
// TurnInput — prepared by agent, executed by runtime
// ---------------------------------------------------------------------------

pub(in crate::agent) struct TurnInput {
    pub options: EngineOptions,
    pub history: Vec<evot_engine::AgentMessage>,
    pub input: Vec<evot_engine::Content>,
    pub session: Arc<Session>,
}

// ---------------------------------------------------------------------------
// TurnFactory — caller-provided builder of per-turn engine state
// ---------------------------------------------------------------------------

/// Rebuilds the engine input for the next internal turn.
///
/// The runtime calls this once per turn. The factory captures whatever
/// the agent layer needs (config, sandbox, tools, system prompt) and
/// resolves the latest history + goal state at call time.
#[async_trait]
pub(in crate::agent) trait TurnFactory: Send + Sync + 'static {
    /// Build the engine options + history + initial user input for this turn.
    async fn build(&self, input: Vec<evot_engine::Content>) -> Result<TurnInput>;
}

// ---------------------------------------------------------------------------
// execute_run — public entry point: schedule a Run on a background task
// ---------------------------------------------------------------------------

pub(in crate::agent) struct ExecuteRunArgs {
    pub run_id: String,
    pub session_id: String,
    pub session: Arc<Session>,
    pub initial_input: Vec<evot_engine::Content>,
    pub factory: Arc<dyn TurnFactory>,
    pub on_complete: Option<Arc<dyn Fn() + Send + Sync>>,
    pub telemetry: Option<crate::telemetry::config::TelemetryConfig>,
    /// Closure that sends an eval prompt to the model and returns the raw text.
    /// Used by the goal evaluator.
    pub eval_fn: Option<EvalFn>,
}

pub(in crate::agent) fn execute_run(args: ExecuteRunArgs) -> Run {
    let (tx, rx) = mpsc::unbounded_channel();
    let control = RunControl::new();
    let run = Run::new(
        args.run_id.clone(),
        args.session_id.clone(),
        rx,
        control.clone(),
    );
    tokio::spawn(run_loop(args, tx, control));
    run
}

// ---------------------------------------------------------------------------
// RuntimeEvent — private orchestration signal
// ---------------------------------------------------------------------------

enum RuntimeEvent {
    Public(RunEventPayload),
    Transcript(TranscriptItem),
    TurnStarted,
    TurnEnded,
    EngineCompleted {
        last_text: String,
        usage: UsageSummary,
        transcript_count: usize,
    },
    Compacted {
        level: u8,
        transcripts: Vec<TranscriptItem>,
    },
}

// ---------------------------------------------------------------------------
// run_loop — outer loop: drive engine turns and consult goal policy between
// ---------------------------------------------------------------------------

async fn run_loop(args: ExecuteRunArgs, tx: mpsc::UnboundedSender<RunEvent>, control: RunControl) {
    let ExecuteRunArgs {
        run_id,
        session_id,
        session,
        initial_input,
        factory,
        on_complete,
        telemetry,
        eval_fn,
    } = args;

    let started_at = Instant::now();
    let _ = tx.send(RunEventContext::new(&run_id, &session_id, 0).started());

    let mut total_usage = UsageSummary::default();
    let mut total_turns: u32 = 0;
    let mut total_transcripts: usize = 0;
    let mut last_text = String::new();
    let mut compact_records: Vec<CompactRecord> = Vec::new();
    let mut next_input = Some(initial_input);
    while let Some(input) = next_input.take() {
        if control.is_cancelled() {
            break;
        }

        let outcome = match factory.build(input).await {
            Ok(turn) => {
                drive_one_turn(
                    turn,
                    &tx,
                    &control,
                    &run_id,
                    &session_id,
                    started_at,
                    telemetry.as_ref(),
                )
                .await
            }
            Err(e) => {
                tracing::error!(
                    stage = "run",
                    status = "build_turn_failed",
                    run_id = %run_id,
                    session_id = %session_id,
                    error = %e,
                );
                break;
            }
        };

        let TurnOutcome {
            turn_count,
            usage,
            transcript_count,
            last_text: turn_last_text,
            compact_records: turn_compacts,
            engine_completed,
            tool_summary,
        } = outcome;

        total_turns = total_turns.saturating_add(turn_count);
        total_usage.input = total_usage.input.saturating_add(usage.input);
        total_usage.output = total_usage.output.saturating_add(usage.output);
        total_usage.cache_read = total_usage.cache_read.saturating_add(usage.cache_read);
        total_usage.cache_write = total_usage.cache_write.saturating_add(usage.cache_write);
        total_transcripts = total_transcripts.saturating_add(transcript_count);
        if !turn_last_text.is_empty() {
            last_text = turn_last_text;
        }
        compact_records.extend(turn_compacts);

        if !engine_completed || control.is_cancelled() {
            break;
        }

        match crate::agent::goal::runtime::after_turn(&session, eval_fn.as_ref(), GoalTurnReport {
            last_text: &last_text,
            tool_summary: &tool_summary,
            usage: &usage,
            elapsed: started_at.elapsed(),
        })
        .await
        {
            Ok(AfterTurn::Continue(input)) => next_input = Some(input),
            Ok(AfterTurn::Stop) => break,
            Err(e) => {
                tracing::warn!(error = %e, "goal step failed");
                break;
            }
        }
    }

    // Emit the aggregated run-finished event and a single transcript stats
    // record so consumers see one Run, not N internal turns.
    let duration_ms = started_at.elapsed().as_millis() as u64;
    let stats = TranscriptStats::RunFinished(RunFinishedStats {
        usage: total_usage.clone(),
        turn_count: total_turns,
        duration_ms,
        transcript_count: total_transcripts,
    });
    if let Err(e) = session.write_items(vec![stats.to_item()]).await {
        tracing::warn!(
            stage = "run",
            status = "stats_persist_failed",
            run_id = %run_id,
            session_id = %session_id,
            error = %e,
        );
    }
    let _ = session.save().await;

    let finished = RunEventContext::new(&run_id, &session_id, total_turns).finished(
        last_text,
        total_usage,
        total_turns,
        duration_ms,
        total_transcripts,
        compact_records,
    );
    let _ = tx.send(finished);
    drop(tx);

    tracing::info!(
        stage = "run",
        status = "finished",
        run_id = %run_id,
        session_id = %session_id,
        elapsed_ms = duration_ms,
        turn = total_turns,
    );

    if let Some(f) = on_complete {
        f();
    }
}

// ---------------------------------------------------------------------------
// drive_one_turn — single engine submit: forward events, persist transcripts
// ---------------------------------------------------------------------------

struct TurnOutcome {
    turn_count: u32,
    usage: UsageSummary,
    transcript_count: usize,
    last_text: String,
    /// Compact records produced during this engine turn, in order.
    compact_records: Vec<CompactRecord>,
    /// True if the engine reached `AgentEnd`; false on abort or channel close.
    engine_completed: bool,
    /// Summarized tool calls from this turn (non-empty means tools were used).
    tool_summary: String,
}

async fn drive_one_turn(
    turn: TurnInput,
    tx: &mpsc::UnboundedSender<RunEvent>,
    control: &RunControl,
    run_id: &str,
    session_id: &str,
    _started_at: Instant,
    telemetry: Option<&crate::telemetry::config::TelemetryConfig>,
) -> TurnOutcome {
    let TurnInput {
        options,
        history,
        input,
        session,
    } = turn;

    let mut engine = build_agent(options, history);
    let user_msg = evot_engine::AgentMessage::Llm(evot_engine::Message::User {
        content: input.clone(),
        timestamp: evot_engine::now_ms(),
    });
    let (engine_handle, engine_rx) = engine.submit(vec![user_msg]).await;
    control.install_engine(engine_handle);

    let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
    let rid = run_id.to_string();
    let sid = session_id.to_string();
    let otel_sub =
        telemetry.and_then(|cfg| crate::telemetry::subscriber::TelemetrySubscriber::new(cfg, &sid));
    tokio::spawn(async move {
        forward_events(engine_rx, runtime_tx, &rid, &sid, otel_sub).await;
    });

    // First user content is part of this turn's transcript record.
    let mut turn_transcripts: Vec<TranscriptItem> = vec![TranscriptItem::user_from_content(&input)];
    let mut saved_count: usize = 0;
    let mut turn_count: u32 = 0;
    let mut tool_calls: Vec<String> = Vec::new();
    let mut outcome = TurnOutcome {
        turn_count: 0,
        usage: UsageSummary::default(),
        transcript_count: 0,
        last_text: String::new(),
        compact_records: Vec::new(),
        engine_completed: false,
        tool_summary: String::new(),
    };

    let flush = |session: &Arc<Session>, items: &[TranscriptItem], saved: &mut usize| {
        let new_items = items[*saved..].to_vec();
        let session = Arc::clone(session);
        *saved = items.len();
        async move {
            if !new_items.is_empty() {
                session.write_items(new_items).await
            } else {
                Ok(())
            }
        }
    };

    while let Some(event) = runtime_rx.recv().await {
        match event {
            RuntimeEvent::TurnStarted => {
                turn_count = turn_count.saturating_add(1);
                session.increment_turn().await;
            }
            RuntimeEvent::Transcript(item) => {
                // Track tool usage for goal completion detection.
                if let TranscriptItem::ToolResult { tool_name, .. } = &item {
                    tool_calls.push(tool_name.clone());
                }
                turn_transcripts.push(item);
            }
            RuntimeEvent::Compacted { level, transcripts } => {
                if level > 0 {
                    let item = TranscriptItem::Marker {
                        kind: crate::types::MarkerKind::Compact,
                        target_seq: None,
                        messages: transcripts,
                    };
                    if let Some(record) = compact_record_from_marker(&item) {
                        outcome.compact_records.push(record);
                    }
                    turn_transcripts.push(item);
                }
            }
            RuntimeEvent::TurnEnded => {
                if let Err(e) = flush(&session, &turn_transcripts, &mut saved_count).await {
                    tracing::error!(
                        stage = "run",
                        status = "incremental_save_failed",
                        run_id = %run_id,
                        session_id = %session_id,
                        error = %e,
                    );
                }
            }
            RuntimeEvent::EngineCompleted {
                last_text,
                usage,
                transcript_count,
            } => {
                outcome.turn_count = turn_count;
                outcome.usage = usage;
                outcome.transcript_count = transcript_count;
                outcome.last_text = last_text;
                outcome.engine_completed = true;
                outcome.tool_summary = tool_calls.join("\n");
                break;
            }
            RuntimeEvent::Public(payload) => {
                let event = RunEventContext::new(run_id, session_id, turn_count).event(payload);
                if tx.send(event).is_err() {
                    // Consumer dropped — bail out; outer loop will stop.
                    break;
                }
            }
        }
    }

    // Final flush in case engine ended without a TurnEnded event.
    if let Err(e) = flush(&session, &turn_transcripts, &mut saved_count).await {
        tracing::warn!(
            stage = "run",
            status = "final_flush_failed",
            run_id = %run_id,
            session_id = %session_id,
            error = %e,
        );
    }

    control.detach_engine();
    if outcome.turn_count == 0 {
        outcome.turn_count = turn_count;
    }
    outcome
}

// ---------------------------------------------------------------------------
// forward_events — AgentEvent → RuntimeEvent (one-step conversion)
// ---------------------------------------------------------------------------

async fn forward_events(
    mut engine_rx: mpsc::UnboundedReceiver<evot_engine::AgentEvent>,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    run_id: &str,
    session_id: &str,
    mut otel: Option<crate::telemetry::subscriber::TelemetrySubscriber>,
) {
    if let Some(ref mut sub) = otel {
        sub.on_agent_start();
    }
    while let Some(event) = engine_rx.recv().await {
        // Drive OTel subscriber
        if let Some(ref mut sub) = otel {
            drive_otel(sub, &event);
        }
        let runtime_events = map_agent_event(&event, run_id, session_id);
        for re in runtime_events {
            if tx.send(re).is_err() {
                break;
            }
        }
    }
    if let Some(ref mut sub) = otel {
        sub.on_agent_end();
    }
}

/// Drive the OTel subscriber from an AgentEvent.
fn drive_otel(
    sub: &mut crate::telemetry::subscriber::TelemetrySubscriber,
    event: &evot_engine::AgentEvent,
) {
    match event {
        evot_engine::AgentEvent::LlmCallStart {
            turn,
            attempt,
            request,
            provider_name,
            server_address,
            server_port,
            ..
        } => {
            sub.on_llm_call_start(
                *turn,
                *attempt,
                &request.model,
                provider_name,
                server_address.as_deref(),
                *server_port,
                request.max_tokens,
                request.temperature,
                &request.messages,
                &request.tools,
            );
        }
        evot_engine::AgentEvent::LlmCallEnd {
            turn,
            attempt,
            usage,
            error,
            metrics,
            stop_reason,
            content,
            ..
        } => {
            let finish_reason = match stop_reason {
                evot_engine::StopReason::Stop => "stop",
                evot_engine::StopReason::ToolUse => "tool_calls",
                evot_engine::StopReason::Length => "length",
                evot_engine::StopReason::Error => "error",
                evot_engine::StopReason::Aborted => "error",
            };
            sub.on_llm_call_end(
                *turn,
                *attempt,
                None, // response_model set from Message, not available here
                usage.input,
                usage.output,
                usage.cache_read,
                usage.cache_write,
                Some(finish_reason),
                error.as_deref(),
                metrics.ttft_ms,
                stop_reason,
                content,
            );
        }
        evot_engine::AgentEvent::LlmCallRetry { .. } => {}
        evot_engine::AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
            ..
        } => {
            sub.on_tool_start(tool_call_id, tool_name, args);
        }
        evot_engine::AgentEvent::ToolExecutionEnd {
            tool_call_id,
            result,
            is_error,
            duration_ms,
            ..
        } => {
            sub.on_tool_end(
                tool_call_id,
                *is_error,
                *duration_ms,
                Some(&serde_json::json!(result)),
            );
        }
        _ => {}
    }
}

/// Map a single `AgentEvent` to zero or more `RuntimeEvent`s.
fn map_agent_event(
    event: &evot_engine::AgentEvent,
    _run_id: &str,
    _session_id: &str,
) -> Vec<RuntimeEvent> {
    match event {
        evot_engine::AgentEvent::AgentStart => vec![],

        evot_engine::AgentEvent::AgentEnd { messages } => {
            let transcripts = from_agent_messages(messages);
            let usage = total_usage(messages);
            let transcript_count = messages.len();

            let last_text = transcripts
                .iter()
                .rev()
                .find_map(|t| {
                    if let TranscriptItem::Assistant { text, .. } = t {
                        if !text.is_empty() {
                            return Some(text.clone());
                        }
                    }
                    None
                })
                .unwrap_or_default();

            vec![RuntimeEvent::EngineCompleted {
                last_text,
                usage,
                transcript_count,
            }]
        }

        evot_engine::AgentEvent::TurnStart => {
            vec![
                RuntimeEvent::TurnStarted,
                RuntimeEvent::Public(RunEventPayload::TurnStarted {}),
            ]
        }

        evot_engine::AgentEvent::TurnEnd { .. } => {
            vec![RuntimeEvent::TurnEnded]
        }

        evot_engine::AgentEvent::MessageStart { .. } => vec![],

        evot_engine::AgentEvent::MessageUpdate {
            delta: evot_engine::StreamDelta::Text { delta },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            delta: Some(delta.clone()),
            thinking_delta: None,
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta: evot_engine::StreamDelta::Thinking { delta },
            ..
        } => vec![RuntimeEvent::Public(RunEventPayload::AssistantDelta {
            delta: None,
            thinking_delta: Some(delta.clone()),
        })],

        evot_engine::AgentEvent::MessageUpdate {
            delta: evot_engine::StreamDelta::ToolCallDelta { .. },
            ..
        } => vec![],

        evot_engine::AgentEvent::MessageEnd { message } => {
            if let evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant {
                content,
                usage,
                stop_reason,
                error_message,
                ..
            }) = message
            {
                let blocks = assistant_blocks_from_content(content);
                let usage_summary = UsageSummary {
                    input: usage.input,
                    output: usage.output,
                    cache_read: usage.cache_read,
                    cache_write: usage.cache_write,
                };
                let transcript_item =
                    transcript_from_assistant_completed(&blocks, &stop_reason.to_string());

                vec![
                    RuntimeEvent::Transcript(transcript_item),
                    RuntimeEvent::Public(RunEventPayload::AssistantCompleted {
                        content: blocks,
                        usage: Some(usage_summary),
                        stop_reason: stop_reason.to_string(),
                        error_message: error_message.clone(),
                    }),
                ]
            } else {
                vec![]
            }
        }

        evot_engine::AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
            preview_command,
        } => vec![RuntimeEvent::Public(RunEventPayload::ToolStarted {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            args: args.clone(),
            preview_command: preview_command.clone(),
        })],

        evot_engine::AgentEvent::ToolExecutionUpdate {
            tool_call_id,
            tool_name,
            partial_result,
        } => {
            let text = extract_content_text(&partial_result.content);
            vec![RuntimeEvent::Public(RunEventPayload::ToolProgress {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                text,
            })]
        }

        evot_engine::AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
            result_tokens,
            duration_ms,
        } => {
            let content = extract_content_text(&result.content);
            vec![
                RuntimeEvent::Transcript(TranscriptItem::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                }),
                RuntimeEvent::Transcript(
                    TranscriptStats::ToolFinished(ToolFinishedStats {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        result_tokens: *result_tokens,
                        duration_ms: *duration_ms,
                        is_error: *is_error,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ToolFinished {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content,
                    is_error: *is_error,
                    details: result.details.clone(),
                    result_tokens: *result_tokens,
                    duration_ms: *duration_ms,
                }),
            ]
        }

        evot_engine::AgentEvent::ProgressMessage {
            tool_call_id,
            tool_name,
            text,
        } => vec![RuntimeEvent::Public(RunEventPayload::ToolProgress {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            text: text.clone(),
        })],

        evot_engine::AgentEvent::Error { error } => {
            vec![RuntimeEvent::Public(RunEventPayload::Error {
                message: error.message.clone(),
            })]
        }

        evot_engine::AgentEvent::LlmCallStart {
            turn,
            attempt,
            injected_count,
            request,
            stats,
            budget,
            provider_name: _,
            server_address: _,
            server_port: _,
        } => {
            let message_count = request.messages.len();
            let tool_count = request.tools.len();

            // Compute message_bytes for transcript (still needs serialization)
            let message_bytes: usize = request
                .messages
                .iter()
                .map(|msg| serialize_or_placeholder(msg, "message").to_string().len())
                .sum();

            let message_stats = Some(LlmMessageStats::from(stats.clone()));

            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallStarted(LlmCallStartedStats {
                        turn: *turn,
                        attempt: *attempt,
                        injected_count: *injected_count,
                        model: request.model.clone(),
                        message_count,
                        message_bytes,
                        system_prompt_tokens: budget.system_prompt_tokens,
                        tool_definition_tokens: budget.tool_definition_tokens,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallStarted {
                    turn: *turn,
                    attempt: *attempt,
                    injected_count: *injected_count,
                    model: request.model.clone(),
                    message_count,
                    message_bytes,
                    estimated_context_tokens: budget.estimated_tokens,
                    system_prompt_tokens: budget.system_prompt_tokens,
                    tool_definition_tokens: budget.tool_definition_tokens,
                    tool_count,
                    message_stats,
                    budget_tokens: budget.budget_tokens,
                    context_window: budget.context_window,
                }),
            ]
        }

        evot_engine::AgentEvent::LlmCallRetry {
            turn,
            attempt,
            max_retries,
            delay_ms,
            error,
        } => {
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallRetry(LlmCallRetryStats {
                        turn: *turn,
                        attempt: *attempt,
                        max_retries: *max_retries,
                        delay_ms: *delay_ms,
                        error: error.clone(),
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallRetry {
                    turn: *turn,
                    attempt: *attempt,
                    max_retries: *max_retries,
                    delay_ms: *delay_ms,
                    error: error.clone(),
                }),
            ]
        }

        evot_engine::AgentEvent::LlmCallEnd {
            turn,
            attempt,
            usage,
            error,
            metrics,
            context_window,
            stop_reason,
            content,
            response_model: _,
            response_id: _,
        } => {
            let usage_summary = UsageSummary {
                input: usage.input,
                output: usage.output,
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
            };
            let llm_metrics = LlmCallMetrics {
                duration_ms: metrics.duration_ms,
                ttfb_ms: metrics.ttfb_ms,
                ttft_ms: metrics.ttft_ms,
                streaming_ms: metrics.streaming_ms,
                chunk_count: metrics.chunk_count,
            };
            // Extract tool call summaries for the public event
            let tool_calls: Vec<LlmToolCallSummary> = content
                .iter()
                .filter_map(|c| match c {
                    evot_engine::Content::ToolCall {
                        id,
                        name,
                        arguments,
                    } => Some(LlmToolCallSummary {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    }),
                    _ => None,
                })
                .collect();
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
                        turn: *turn,
                        attempt: *attempt,
                        usage: usage_summary.clone(),
                        metrics: Some(llm_metrics.clone()),
                        error: error.clone(),
                        context_window: *context_window,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::LlmCallCompleted {
                    turn: *turn,
                    attempt: *attempt,
                    usage: usage_summary.clone(),
                    cache_read: usage_summary.cache_read,
                    cache_write: usage_summary.cache_write,
                    error: error.clone(),
                    metrics: Some(llm_metrics),
                    context_window: *context_window,
                    stop_reason: stop_reason.to_string(),
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                }),
            ]
        }

        evot_engine::AgentEvent::ContextCompactionStart {
            message_count,
            budget,
            message_stats,
        } => {
            let stats = Some(LlmMessageStats::from(message_stats.clone()));
            vec![
                RuntimeEvent::Transcript(
                    TranscriptStats::ContextCompactionStarted(ContextCompactionStartedStats {
                        message_count: *message_count,
                        estimated_tokens: budget.estimated_tokens,
                        budget_tokens: budget.budget_tokens,
                        system_prompt_tokens: budget.system_prompt_tokens,
                        tool_definition_tokens: budget.tool_definition_tokens,
                        context_window: budget.context_window,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ContextCompactionStarted {
                    message_count: *message_count,
                    estimated_tokens: budget.estimated_tokens,
                    budget_tokens: budget.budget_tokens,
                    system_prompt_tokens: budget.system_prompt_tokens,
                    tool_definition_tokens: budget.tool_definition_tokens,
                    context_window: budget.context_window,
                    message_stats: stats,
                }),
            ]
        }

        evot_engine::AgentEvent::ContextCompactionEnd {
            stats,
            messages,
            context_window,
        } => {
            let compacted_transcripts = from_agent_messages(messages);

            let result = if stats.level > 0 {
                crate::types::CompactionResult::LevelCompacted {
                    level: stats.level,
                    before_message_count: stats.before_message_count,
                    after_message_count: stats.after_message_count,
                    before_estimated_tokens: stats.before_estimated_tokens,
                    after_estimated_tokens: stats.after_estimated_tokens,
                    tool_outputs_truncated: stats.tool_outputs_truncated,
                    turns_summarized: stats.turns_summarized,
                    messages_dropped: stats.messages_dropped,
                    oversize_capped: stats.oversize_capped,
                    age_cleared: stats.age_cleared,
                    actions: crate::types::convert_compaction_actions(&stats.actions),
                }
            } else if stats.current_run_cleared > 0 {
                crate::types::CompactionResult::RunOnceCleared {
                    cleared_count: stats.current_run_cleared,
                    before_message_count: stats.before_message_count,
                    before_estimated_tokens: stats.before_estimated_tokens,
                    after_estimated_tokens: stats.after_estimated_tokens,
                    saved_tokens: stats
                        .before_estimated_tokens
                        .saturating_sub(stats.after_estimated_tokens),
                    actions: crate::types::convert_compaction_actions(&stats.actions),
                }
            } else {
                crate::types::CompactionResult::NoOp
            };

            vec![
                RuntimeEvent::Compacted {
                    level: stats.level,
                    transcripts: compacted_transcripts,
                },
                RuntimeEvent::Transcript(
                    TranscriptStats::ContextCompactionCompleted(ContextCompactionCompletedStats {
                        result: result.clone(),
                        context_window: *context_window,
                    })
                    .to_item(),
                ),
                RuntimeEvent::Public(RunEventPayload::ContextCompactionCompleted {
                    result,
                    context_window: *context_window,
                }),
            ]
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn serialize_or_placeholder<T: serde::Serialize>(value: &T, kind: &str) -> serde_json::Value {
    match serde_json::to_value(value) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("failed to serialize {kind}: {e}");
            serde_json::json!({
                "type": "serialization_error",
                "kind": kind,
                "message": e.to_string(),
            })
        }
    }
}

/// Extract a compact record from a single transcript item.
fn compact_record_from_marker(item: &TranscriptItem) -> Option<CompactRecord> {
    use crate::agent::run::observability::compact_record_from_result;

    let stats = TranscriptStats::try_from_item(item)?;
    match stats {
        TranscriptStats::ContextCompactionCompleted(s) => compact_record_from_result(&s.result),
        _ => None,
    }
}

pub(crate) fn build_agent(
    options: EngineOptions,
    prior_messages: Vec<evot_engine::AgentMessage>,
) -> evot_engine::Agent {
    use evot_engine::provider::AnthropicProvider;
    use evot_engine::provider::ModelConfig;
    use evot_engine::provider::OpenAiCompat;
    use evot_engine::provider::OpenAiCompatProvider;

    let mut model_config = match options.protocol {
        Protocol::Anthropic => ModelConfig::anthropic(&options.model, &options.model),
        Protocol::OpenAi => {
            let mut mc = ModelConfig::local("", &options.model);
            mc.compat = Some(match options.provider.as_str() {
                "openai" => OpenAiCompat::openai(),
                "deepseek" => OpenAiCompat::deepseek(),
                "xai" => OpenAiCompat::xai(),
                "groq" => OpenAiCompat::groq(),
                "cerebras" => OpenAiCompat::cerebras(),
                "openrouter" => OpenAiCompat::openrouter(),
                "mistral" => OpenAiCompat::mistral(),
                "zai" => OpenAiCompat::zai(),
                "minimax" => OpenAiCompat::minimax(),
                _ => OpenAiCompat::default(),
            });
            mc
        }
    };
    if let Some(base_url) = &options.base_url {
        model_config.base_url = base_url.clone();
    }

    if options.protocol == Protocol::OpenAi {
        if let Some(compat) = &mut model_config.compat {
            compat.caps |= options.compat_caps;
        }
    }

    let provider_agent = match options.protocol {
        Protocol::Anthropic => evot_engine::Agent::new(AnthropicProvider),
        Protocol::OpenAi => evot_engine::Agent::new(OpenAiCompatProvider),
    };

    let limits = evot_engine::context::ExecutionLimits {
        max_turns: options.limits.max_turns as usize,
        max_total_tokens: options.limits.max_total_tokens as usize,
        max_duration: std::time::Duration::from_secs(options.limits.max_duration_secs),
    };

    let skills = match crate::agent::prompt::skill::load_skills(&options.skills_dirs) {
        Ok(specs) => evot_engine::SkillSet::new(specs),
        Err(e) => {
            tracing::warn!("failed to load skills: {e}");
            evot_engine::SkillSet::empty()
        }
    };

    provider_agent
        .with_model(&options.model)
        .with_api_key(&options.api_key)
        .with_model_config(model_config)
        .with_system_prompt(&options.system_prompt)
        .with_messages(prior_messages)
        .with_execution_limits(limits)
        .with_tools(options.tools)
        .with_cwd(options.cwd)
        .with_path_guard(options.path_guard)
        .with_skills(skills)
        .with_thinking(options.thinking_level)
        .with_prompt_cache_key_opt(options.prompt_cache_key)
        .with_spill_opt(
            options
                .spill_dir
                .map(|dir| Arc::new(evot_engine::spill::FsSpill::new(dir))),
        )
}
