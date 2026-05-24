//! Stream an assistant response from the LLM, with retry and SSE forwarding.

use tokio::sync::mpsc;

use super::config::default_convert_to_llm;
use super::config::AgentLoopConfig;
use crate::context::ContextConfig;
use crate::provider::ProviderError;
use crate::provider::StreamConfig;
use crate::provider::StreamEvent;
use crate::provider::ThinkingPassbackPolicy;
use crate::provider::ToolDefinition;
use crate::types::*;

const REQUEST_TOOL_RESULT_MAX_BYTES: usize = 16_000;
const REQUEST_TOOL_RESULT_MAX_LINES: usize = 80;
const REQUEST_OLD_TOOL_RESULTS_TOTAL_MAX_BYTES: usize = 64_000;

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
    let llm_messages =
        compact_tool_results_for_request_view(llm_messages, config.context_config.as_ref());

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
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        })
        .collect();

    // Retry loop for transient provider errors
    let retry = &config.retry_policy;
    let mut attempt = 0;
    let shared_metrics = std::sync::Arc::new(std::sync::Mutex::new(LlmCallMetrics::default()));
    let result = loop {
        let stream_config = StreamConfig {
            model: config.model.clone(),
            system_prompt: context.system_prompt.clone(),
            messages: llm_messages.clone(),
            tools: tool_defs.clone(),
            thinking_level: config.thinking_level,
            api_key: config.api_key.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            model_config: config.model_config.clone(),
            cache_config: config.cache_config.clone(),
            prompt_cache_key: context.prompt_cache_key.clone(),
        };

        // Emit LlmCallStart before each provider attempt
        let llm_stats = crate::context::compute_call_stats(&llm_messages);
        let (otel_provider_name, otel_server_address, otel_server_port) =
            extract_otel_server_info(config);
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
                temperature: config.temperature,
            },
            stats: llm_stats,
            budget: budget.clone(),
            provider_name: otel_provider_name,
            server_address: otel_server_address,
            server_port: otel_server_port,
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

fn compact_tool_results_for_request_view(
    messages: Vec<Message>,
    ctx_config: Option<&ContextConfig>,
) -> Vec<Message> {
    let Some(ctx_config) = ctx_config else {
        return messages;
    };

    let protected_suffix_start = messages.len().saturating_sub(ctx_config.keep_recent);
    let mut compacted: Vec<Message> = messages
        .into_iter()
        .enumerate()
        .map(|(idx, msg)| {
            if idx >= protected_suffix_start {
                return msg;
            }

            match msg {
                Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content,
                    is_error,
                    timestamp,
                    retention,
                } => Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content: shrink_tool_result_content_for_request(content, false),
                    is_error,
                    timestamp,
                    retention,
                },
                other => other,
            }
        })
        .collect();

    let mut old_tool_bytes = old_tool_result_request_bytes(&compacted, protected_suffix_start);
    if old_tool_bytes <= REQUEST_OLD_TOOL_RESULTS_TOTAL_MAX_BYTES {
        return compacted;
    }

    for idx in (0..protected_suffix_start).rev() {
        if old_tool_bytes <= REQUEST_OLD_TOOL_RESULTS_TOTAL_MAX_BYTES {
            break;
        }

        let Message::ToolResult { content, .. } = &mut compacted[idx] else {
            continue;
        };
        let before = tool_result_request_bytes(content);
        if before == 0 {
            continue;
        }

        let original = std::mem::take(content);
        let mut omitted = shrink_tool_result_content_for_request(original, true);
        let after = tool_result_request_bytes(&omitted);
        if after >= before {
            omitted = vec![omitted_tool_result_marker(before)];
        }
        *content = omitted;
        old_tool_bytes = old_tool_result_request_bytes(&compacted, protected_suffix_start);
    }

    compacted
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

fn old_tool_result_request_bytes(messages: &[Message], protected_suffix_start: usize) -> usize {
    messages
        .iter()
        .take(protected_suffix_start)
        .map(|message| match message {
            Message::ToolResult { content, .. } => tool_result_request_bytes(content),
            _ => 0,
        })
        .sum()
}

fn tool_result_request_bytes(content: &[Content]) -> usize {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            Content::Thinking { thinking, .. } => thinking.len(),
            Content::ToolCall { arguments, .. } => arguments.to_string().len(),
            Content::Image {
                source: ImageSource::Base64 { data, .. },
                ..
            } => data.len(),
            Content::Image { .. } => 0,
        })
        .sum()
}

fn omitted_tool_result_marker(bytes: usize) -> Content {
    Content::Text {
        text: format!("[tool result omitted from request view: {bytes} bytes]"),
    }
}

fn omitted_content_marker(kind: &str, bytes: usize) -> Content {
    Content::Text {
        text: format!("[{kind} omitted from old tool result request view: {bytes} bytes]"),
    }
}

fn shrink_tool_result_content_for_request(
    content: Vec<Content>,
    force_compact: bool,
) -> Vec<Content> {
    if force_compact {
        let original_bytes = tool_result_request_bytes(&content);
        let mut kept: Vec<Content> = content
            .into_iter()
            .filter(|c| {
                matches!(c, Content::Image {
                    source: ImageSource::Path { .. },
                    ..
                })
            })
            .collect();
        kept.push(omitted_tool_result_marker(original_bytes));
        return kept;
    }

    content
        .into_iter()
        .map(|c| match c {
            Content::Text { text } => Content::Text {
                text: shrink_tool_result_text_for_request(&text),
            },
            Content::Thinking { thinking, .. } => {
                omitted_content_marker("thinking", thinking.len())
            }
            Content::ToolCall { arguments, .. } => {
                omitted_content_marker("nested tool call", arguments.to_string().len())
            }
            Content::Image {
                mime_type,
                source: ImageSource::Base64 { data, path },
            } => {
                if let Some(path) = path {
                    Content::Image {
                        mime_type,
                        source: ImageSource::Path { path },
                    }
                } else {
                    omitted_content_marker("image", data.len())
                }
            }
            other => other,
        })
        .collect()
}

fn shrink_tool_result_text_for_request(text: &str) -> String {
    let line_capped = crate::context::truncate_text_head_tail(text, REQUEST_TOOL_RESULT_MAX_LINES);
    crate::tools::validation::truncate_tool_text(&line_capped, REQUEST_TOOL_RESULT_MAX_BYTES)
}

/// Extract OTel-standard provider name and server address/port from config.
fn extract_otel_server_info(config: &AgentLoopConfig) -> (String, Option<String>, Option<u16>) {
    let (provider_name, base_url) = match &config.model_config {
        Some(mc) => {
            let pn = normalize_provider_name(&mc.provider, &mc.base_url);
            (pn, Some(mc.base_url.as_str()))
        }
        None => ("unknown".to_string(), None),
    };

    let (address, port) = match base_url {
        Some(url) => parse_host_port(url),
        None => (None, None),
    };

    (provider_name, address, port)
}

/// Parse host and port from a URL string without external crate.
fn parse_host_port(url: &str) -> (Option<String>, Option<u16>) {
    // Strip scheme
    let after_scheme = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    // Strip path
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    // Strip userinfo
    let host_port = authority.rsplit('@').next().unwrap_or(authority);

    if let Some(colon_idx) = host_port.rfind(':') {
        let host = &host_port[..colon_idx];
        let port = host_port[colon_idx + 1..].parse::<u16>().ok();
        (Some(host.to_string()), port)
    } else {
        let default_port = if url.starts_with("https") {
            Some(443)
        } else if url.starts_with("http") {
            Some(80)
        } else {
            None
        };
        (Some(host_port.to_string()), default_port)
    }
}

/// Map provider strings to OTel Gen AI standard values.
fn normalize_provider_name(provider: &str, base_url: &str) -> String {
    match provider {
        "anthropic" => "anthropic".to_string(),
        "bedrock" => "aws.bedrock".to_string(),
        "openai" => "openai".to_string(),
        "deepseek" => "deepseek".to_string(),
        "xai" => "xai".to_string(),
        "groq" => "groq".to_string(),
        "mistral" => "mistral".to_string(),
        "zai" => "zai".to_string(),
        "minimax" => "minimax".to_string(),
        "local" => "openai_compatible".to_string(),
        other => {
            // Try to infer from base_url
            if base_url.contains("anthropic.com") {
                "anthropic".to_string()
            } else if base_url.contains("openai.com") {
                "openai".to_string()
            } else if base_url.contains("amazonaws.com") {
                "aws.bedrock".to_string()
            } else {
                other.to_string()
            }
        }
    }
}
