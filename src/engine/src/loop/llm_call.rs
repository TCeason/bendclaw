//! Stream an assistant response from the LLM, with retry and SSE forwarding.

use tokio::sync::mpsc;

use super::config::default_convert_to_llm;
use super::config::AgentLoopConfig;
use crate::provider::ProviderError;
use crate::provider::StreamConfig;
use crate::provider::StreamEvent;
use crate::provider::ThinkingPassbackPolicy;
use crate::provider::ToolDefinition;
use crate::types::*;

pub(super) struct AssistantStreamResult {
    pub message: Message,
}

impl AssistantStreamResult {
    fn complete(message: Message) -> Self {
        Self { message }
    }
}

/// Stream an assistant response from the LLM.
pub(super) async fn stream_assistant_response(
    context: &AgentContext,
    config: &AgentLoopConfig,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
    turn: usize,
    injected_count: usize,
    budget: crate::context::ContextBudgetSnapshot,
) -> AssistantStreamResult {
    // Apply context transform
    let messages = if let Some(transform) = &config.transform_context {
        transform(context.messages.clone())
    } else {
        context.messages.clone()
    };

    // Convert to LLM messages
    let convert = config.convert_to_llm.as_ref();
    let llm_messages = match convert {
        Some(f) => f(&messages),
        None => default_convert_to_llm(&messages),
    };
    // Strip thinking blocks before sending history back to the provider.
    // Most providers do not require thinking passback, and keeping it bloats
    // context/cache keys. DeepSeek-compatible Anthropic endpoints require
    // thinking on retained assistant tool-use messages.
    let llm_messages = strip_thinking(llm_messages, thinking_passback_policy(config));

    // Build tool definitions
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

    // Retry loop for transient provider errors
    let retry = &config.retry_policy;
    let mut attempt = 0;
    let shared_metrics = std::sync::Arc::new(std::sync::Mutex::new(LlmCallMetrics::default()));
    let result = loop {
        // Temperature is incompatible with extended thinking — the Anthropic
        // API requires temperature=1 (the default) when thinking is enabled.
        // Suppress any user-configured temperature to avoid API errors.
        let effective_temperature = if config.thinking_level == ThinkingLevel::Off {
            config.temperature
        } else {
            None
        };

        let stream_config = StreamConfig {
            model: config.model.clone(),
            system_prompt: context.system_prompt.clone(),
            messages: llm_messages.clone(),
            tools: tool_defs.clone(),
            thinking_level: config.thinking_level,
            api_key: config.api_key.clone(),
            max_tokens: config.max_tokens,
            temperature: effective_temperature,
            model_config: config.model_config.clone(),
            cache_config: config.cache_config.clone(),
            prompt_cache_key: context.prompt_cache_key.clone(),
        };

        // Emit LlmCallStart before each provider attempt
        let llm_stats = crate::context::compute_call_stats(&llm_messages);
        tx.send(AgentEvent::LlmCallStart {
            turn,
            attempt,
            injected_count,
            request: LlmCallRequest {
                model: config.model.clone(),
                system_prompt: context.system_prompt.clone(),
                messages: llm_messages.clone(),
                tools: tool_defs.clone(),
                max_tokens: config.max_tokens,
                temperature: effective_temperature,
            },
            stats: llm_stats,
            budget: budget.clone(),
        })
        .ok();

        let call_start = std::time::Instant::now();
        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
        let provider_cancel = cancel.clone();

        // Reset metrics for this attempt.
        if let Ok(mut m) = shared_metrics.lock() {
            *m = LlmCallMetrics::default();
        }
        let metrics_handle = shared_metrics.clone();

        // Spawn a task to forward events in real-time as the provider streams
        let event_tx = tx.clone();
        let model_for_events = config.model.clone();
        let forward_handle = tokio::spawn(async move {
            let mut partial_message: Option<AgentMessage> = None;
            let mut first_delta_seen = false;
            let mut chunk_count: u64 = 0;
            while let Some(event) = stream_rx.recv().await {
                match &event {
                    StreamEvent::Start => {
                        if let Ok(mut m) = metrics_handle.lock() {
                            m.ttfb_ms = call_start.elapsed().as_millis() as u64;
                        }
                        let placeholder = AgentMessage::Llm(Message::Assistant {
                            content: Vec::new(),
                            stop_reason: StopReason::Stop,
                            model: model_for_events.clone(),
                            provider: String::new(),
                            usage: Usage::default(),
                            timestamp: now_ms(),
                            error_message: None,
                            response_id: None,
                        });
                        partial_message = Some(placeholder.clone());
                        event_tx
                            .send(AgentEvent::MessageStart {
                                message: placeholder,
                            })
                            .ok();
                    }
                    StreamEvent::TextDelta { delta, .. } => {
                        if !first_delta_seen {
                            first_delta_seen = true;
                            if let Ok(mut m) = metrics_handle.lock() {
                                m.ttft_ms = call_start.elapsed().as_millis() as u64;
                            }
                        }
                        chunk_count += 1;
                        if let Some(ref msg) = partial_message {
                            event_tx
                                .send(AgentEvent::MessageUpdate {
                                    message: msg.clone(),
                                    delta: StreamDelta::Text {
                                        delta: delta.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::ThinkingDelta { delta, .. } => {
                        if !first_delta_seen {
                            first_delta_seen = true;
                            if let Ok(mut m) = metrics_handle.lock() {
                                m.ttft_ms = call_start.elapsed().as_millis() as u64;
                            }
                        }
                        chunk_count += 1;
                        if let Some(ref msg) = partial_message {
                            event_tx
                                .send(AgentEvent::MessageUpdate {
                                    message: msg.clone(),
                                    delta: StreamDelta::Thinking {
                                        delta: delta.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::ToolCallDelta { delta, .. } => {
                        chunk_count += 1;
                        if let Some(ref msg) = partial_message {
                            event_tx
                                .send(AgentEvent::MessageUpdate {
                                    message: msg.clone(),
                                    delta: StreamDelta::ToolCallDelta {
                                        delta: delta.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::Done { message } => {
                        let elapsed = call_start.elapsed().as_millis() as u64;
                        if let Ok(mut m) = metrics_handle.lock() {
                            m.duration_ms = elapsed;
                            if first_delta_seen {
                                m.streaming_ms = elapsed.saturating_sub(m.ttft_ms);
                            }
                            m.chunk_count = chunk_count;
                        }
                        let am: AgentMessage = message.clone().into();
                        partial_message = Some(am.clone());
                        event_tx.send(AgentEvent::MessageEnd { message: am }).ok();
                    }
                    StreamEvent::Error { message } => {
                        if let Ok(mut m) = metrics_handle.lock() {
                            m.duration_ms = call_start.elapsed().as_millis() as u64;
                            if first_delta_seen {
                                m.streaming_ms = m.duration_ms.saturating_sub(m.ttft_ms);
                            }
                            m.chunk_count = chunk_count;
                        }
                        let am: AgentMessage = message.clone().into();
                        if partial_message.is_none() {
                            event_tx
                                .send(AgentEvent::MessageStart {
                                    message: am.clone(),
                                })
                                .ok();
                        }
                        partial_message = Some(am.clone());
                        event_tx.send(AgentEvent::MessageEnd { message: am }).ok();
                    }
                    _ => {}
                }
            }
        });

        // Provider streams concurrently — events are forwarded in real-time
        // When provider returns, stream_tx is dropped, ending the forwarder
        let result = config
            .provider
            .stream(stream_config, stream_tx, provider_cancel)
            .await;

        // Promote empty Ok(Message) to a retryable error so the retry loop
        // handles it uniformly instead of terminating the agent loop.
        let result = match result {
            Ok(ref outcome) => {
                let msg = outcome.message();
                let is_empty = match msg {
                    Message::Assistant {
                        content,
                        usage,
                        stop_reason,
                        ..
                    } => {
                        content.is_empty()
                            && usage.input == 0
                            && usage.output == 0
                            && stop_reason != &StopReason::Error
                    }
                    _ => false,
                };
                if is_empty {
                    Err(ProviderError::Network(
                        "Empty response from provider (no content, no usage)".into(),
                    ))
                } else {
                    result
                }
            }
            err => err,
        };

        match &result {
            Err(e)
                if crate::retry::should_retry(e)
                    && attempt < retry.max_retries()
                    && !cancel.is_cancelled() =>
            {
                // Abort forwarder to prevent forwarding events from failed attempt
                forward_handle.abort();
                let mut error_metrics =
                    shared_metrics.lock().map(|m| m.clone()).unwrap_or_default();
                if error_metrics.duration_ms == 0 {
                    error_metrics.duration_ms = call_start.elapsed().as_millis() as u64;
                }
                // Emit LlmCallEnd for the failed attempt
                tx.send(AgentEvent::LlmCallEnd {
                    turn,
                    attempt,
                    usage: Usage::default(),
                    error: Some(e.to_string()),
                    metrics: error_metrics,
                    context_window: budget.context_window,
                    stop_reason: StopReason::Error,
                    content: vec![],
                    response_model: None,
                    response_id: None,
                })
                .ok();
                attempt += 1;
                let delay = e
                    .retry_after()
                    .unwrap_or_else(|| retry.delay_for_attempt(attempt));
                tx.send(AgentEvent::LlmCallRetry {
                    turn,
                    attempt,
                    max_retries: retry.max_retries(),
                    delay_ms: delay.as_millis() as u64,
                    error: e.to_string(),
                })
                .ok();
                tokio::time::sleep(delay).await;
                continue;
            }
            _ => {
                // Final attempt — wait for forwarder to finish processing remaining events
                let _ = forward_handle.await;
                if let Ok(mut m) = shared_metrics.lock() {
                    if m.duration_ms == 0 {
                        m.duration_ms = call_start.elapsed().as_millis() as u64;
                    }
                }
                break result;
            }
        }
    };

    let collected_metrics: LlmCallMetrics =
        shared_metrics.lock().map(|m| m.clone()).unwrap_or_default();

    match result {
        Ok(outcome) => {
            let msg = outcome.message();
            let (usage, stop_reason, content, response_model, response_id) = match msg {
                Message::Assistant {
                    usage,
                    stop_reason,
                    content,
                    model,
                    response_id,
                    ..
                } => (
                    usage.clone(),
                    stop_reason.clone(),
                    content.clone(),
                    Some(model.clone()),
                    response_id.clone(),
                ),
                _ => (Usage::default(), StopReason::Stop, vec![], None, None),
            };

            tx.send(AgentEvent::LlmCallEnd {
                turn,
                attempt,
                usage,
                error: None,
                metrics: collected_metrics,
                context_window: budget.context_window,
                stop_reason,
                content,
                response_model,
                response_id,
            })
            .ok();
            AssistantStreamResult::complete(outcome.into_message())
        }
        Err(e) => {
            tx.send(AgentEvent::LlmCallEnd {
                turn,
                attempt,
                usage: Usage::default(),
                error: Some(e.to_string()),
                metrics: collected_metrics,
                context_window: budget.context_window,
                stop_reason: StopReason::Error,
                content: vec![],
                response_model: None,
                response_id: None,
            })
            .ok();
            AssistantStreamResult::complete(Message::Assistant {
                content: vec![Content::Text {
                    text: String::new(),
                }],
                stop_reason: StopReason::Error,
                model: config.model.clone(),
                provider: "unknown".into(),
                usage: Usage::default(),
                timestamp: now_ms(),
                error_message: Some(e.to_string()),
                response_id: None,
            })
        }
    }
}

/// Strip thinking blocks from request history.
///
/// Thinking is the model's internal reasoning. It is expensive to replay and
/// usually not part of the provider protocol. DeepSeek-compatible Anthropic
/// endpoints are the exception: assistant tool-use messages retained in history
/// must keep their original thinking blocks.
fn strip_thinking(messages: Vec<Message>, policy: ThinkingPassbackPolicy) -> Vec<Message> {
    messages
        .into_iter()
        .map(|msg| match policy {
            ThinkingPassbackPolicy::Disabled => strip_message_thinking(msg),
            ThinkingPassbackPolicy::ToolUseMessages if is_assistant_tool_use_message(&msg) => msg,
            ThinkingPassbackPolicy::ToolUseMessages => strip_message_thinking(msg),
        })
        .collect()
}

fn thinking_passback_policy(config: &AgentLoopConfig) -> ThinkingPassbackPolicy {
    config
        .model_config
        .as_ref()
        .map(|model_config| model_config.thinking_passback)
        .unwrap_or_default()
}

fn is_assistant_tool_use_message(message: &Message) -> bool {
    matches!(
        message,
        Message::Assistant { content, .. }
            if content.iter().any(|c| matches!(c, Content::ToolCall { .. }))
    )
}

fn strip_message_thinking(msg: Message) -> Message {
    match msg {
        Message::Assistant {
            content,
            stop_reason,
            model,
            provider,
            usage,
            timestamp,
            error_message,
            response_id,
        } => {
            let filtered: Vec<Content> = content
                .into_iter()
                .filter(|c| !matches!(c, Content::Thinking { .. }))
                .collect();
            // Keep original content if filtering would leave it empty — some
            // providers reject empty content arrays.
            if filtered.is_empty() {
                Message::Assistant {
                    content: vec![Content::Text {
                        text: "(thinking only)".to_string(),
                    }],
                    stop_reason,
                    model,
                    provider,
                    usage,
                    timestamp,
                    error_message,
                    response_id,
                }
            } else {
                Message::Assistant {
                    content: filtered,
                    stop_reason,
                    model,
                    provider,
                    usage,
                    timestamp,
                    error_message,
                    response_id,
                }
            }
        }
        other => other,
    }
}
