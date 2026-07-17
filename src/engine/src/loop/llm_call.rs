//! Stream an assistant response from the LLM, with retry and SSE forwarding.

use tokio::sync::mpsc;

use super::config::default_convert_to_llm;
use super::config::AgentLoopConfig;
use crate::provider::ApiProtocol;
use crate::provider::ProviderError;
use crate::provider::StreamConfig;
use crate::provider::StreamEvent;
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

    // Last-line guard before the request: drop any tool call/result that lacks
    // an adjacent partner. Anthropic-compatible providers reject a tool_result
    // whose tool_use is not in the previous message (HTTP 400). Loaded history is
    // already sanitized at session build, so in the normal path this is a no-op;
    // it stays here so the request is well-formed regardless of how `messages`
    // was assembled (custom transform_context, future call sites).
    let messages = crate::context::sanitize_tool_pairs(messages);

    // Convert to LLM messages
    let convert = config.convert_to_llm.as_ref();
    let llm_messages = match convert {
        Some(f) => f(&messages),
        None => default_convert_to_llm(&messages),
    };
    // Normalize provider-opaque history at the LLM boundary. Replayable
    // thinking survives only for the exact provider/model/protocol that emitted
    // it; foreign thinking is retained as ordinary text context.
    let (target_provider, target_model, target_api) = target_model(config);
    let llm_messages = crate::context::transform_messages_for_model(
        llm_messages,
        &target_provider,
        &target_model,
        target_api,
    );

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
                    StreamEvent::TextDelta {
                        content_index,
                        delta,
                    } => {
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
                                        content_index: *content_index,
                                        delta: delta.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::ThinkingDelta {
                        content_index,
                        delta,
                    } => {
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
                                        content_index: *content_index,
                                        delta: delta.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::ToolCallStart {
                        content_index,
                        id,
                        name,
                    } => {
                        chunk_count += 1;
                        if let Some(ref msg) = partial_message {
                            event_tx
                                .send(AgentEvent::MessageUpdate {
                                    message: msg.clone(),
                                    delta: StreamDelta::ToolCallStart {
                                        content_index: *content_index,
                                        id: id.clone(),
                                        name: name.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::ToolCallDelta {
                        content_index,
                        id,
                        name,
                        delta,
                    } => {
                        chunk_count += 1;
                        if let Some(ref msg) = partial_message {
                            event_tx
                                .send(AgentEvent::MessageUpdate {
                                    message: msg.clone(),
                                    delta: StreamDelta::ToolCallDelta {
                                        content_index: *content_index,
                                        id: id.clone(),
                                        name: name.clone(),
                                        delta: delta.clone(),
                                    },
                                })
                                .ok();
                        }
                    }
                    StreamEvent::ToolCallEnd {
                        content_index,
                        id,
                        name,
                        arguments,
                    } => {
                        chunk_count += 1;
                        if let Some(ref msg) = partial_message {
                            event_tx
                                .send(AgentEvent::MessageUpdate {
                                    message: msg.clone(),
                                    delta: StreamDelta::ToolCallEnd {
                                        content_index: *content_index,
                                        id: id.clone(),
                                        name: name.clone(),
                                        arguments: arguments.clone(),
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
                        if partial_message.is_none() {
                            let am: AgentMessage = message.clone().into();
                            event_tx.send(AgentEvent::MessageStart { message: am }).ok();
                        }
                    }
                    StreamEvent::Error { message } => {
                        if let Ok(mut m) = metrics_handle.lock() {
                            m.duration_ms = call_start.elapsed().as_millis() as u64;
                            if first_delta_seen {
                                m.streaming_ms = m.duration_ms.saturating_sub(m.ttft_ms);
                            }
                            m.chunk_count = chunk_count;
                        }
                        if partial_message.is_none() {
                            let am: AgentMessage = message.clone().into();
                            event_tx.send(AgentEvent::MessageStart { message: am }).ok();
                        }
                    }
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
                        stop_reason,
                        ..
                    } => {
                        !content.iter().any(|block| match block {
                            Content::Text { text } => !text.trim().is_empty(),
                            Content::Thinking { thinking, .. } => !thinking.trim().is_empty(),
                            Content::Image { .. } | Content::ToolCall { .. } => true,
                        }) && stop_reason != &StopReason::Error
                    }
                    _ => false,
                };
                if is_empty {
                    Err(ProviderError::Network(
                        "Empty response from provider (no content)".into(),
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

fn target_model(config: &AgentLoopConfig) -> (String, String, ApiProtocol) {
    match &config.model_config {
        Some(model) => (model.provider.clone(), model.id.clone(), model.api),
        None => (
            "openai".into(),
            config.model.clone(),
            ApiProtocol::OpenAiCompletions,
        ),
    }
}
