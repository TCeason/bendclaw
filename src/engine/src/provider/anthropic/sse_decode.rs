//! Anthropic SSE stream decoding.
//!
//! Parses Anthropic Messages API SSE events and translates them into
//! internal [`StreamEvent`]s while accumulating the final [`Message`].

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::types::*;
use crate::provider::error::classify_sse_error_event;
use crate::provider::error::ProviderError;
use crate::provider::stream_http;
use crate::provider::stream_http::SseEvent;
use crate::provider::traits::StreamConfig;
use crate::provider::traits::StreamEvent;
use crate::provider::traits::StreamOutcome;
use crate::types::*;

/// Drive an Anthropic SSE stream from a raw HTTP response.
///
/// Parses SSE frames, translates Anthropic event types into [`StreamEvent`]s,
/// and returns the final assembled [`Message::Assistant`].
pub(crate) async fn decode_sse_stream(
    response: reqwest::Response,
    tx: mpsc::UnboundedSender<StreamEvent>,
    cancel: CancellationToken,
    config: &StreamConfig,
) -> Result<StreamOutcome, ProviderError> {
    let (sse_tx, mut sse_rx) = mpsc::unbounded_channel::<SseEvent>();

    // Spawn SSE frame parser
    let sse_cancel = cancel.clone();
    let sse_handle =
        tokio::spawn(
            async move { stream_http::drive_sse_response(response, sse_tx, sse_cancel).await },
        );

    let mut content: Vec<Content> = Vec::new();
    let mut usage = Usage::default();
    let mut stop_reason = StopReason::Stop;
    let mut response_id: Option<String> = None;
    let mut incomplete_tool_use_error: Option<ProviderError> = None;

    let _ = tx.send(StreamEvent::Start);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return Err(ProviderError::Cancelled);
            }
            event = sse_rx.recv() => {
                match event {
                    None => break,
                    Some(sse) => {
                        if process_sse_event(
                            &sse,
                            &tx,
                            &mut content,
                            &mut usage,
                            &mut stop_reason,
                            &mut response_id,
                            &mut incomplete_tool_use_error,
                        )? {
                            break;
                        }
                    }
                }
            }
        }
    }

    // Wait for SSE driver to finish.
    // If the driver errored (e.g. network disconnect mid-stream), always
    // propagate — partial content is incomplete and must not be used.
    if let Ok(Err(e)) = sse_handle.await {
        debug!("SSE driver error (content_len={}): {e}", content.len());
        return Err(ProviderError::Network(e));
    }

    // Detect empty response: no content and no usage from provider
    if content.is_empty() && usage.input == 0 && usage.output == 0 {
        return Err(ProviderError::Api(
            "Empty response from provider (no content, no usage)".into(),
        ));
    }

    finalize_tool_call_inputs(&mut content);

    let has_tool_calls = content
        .iter()
        .any(|c| matches!(c, Content::ToolCall { .. }));
    if has_tool_calls {
        stop_reason = StopReason::ToolUse;
    }

    let message = Message::Assistant {
        content,
        stop_reason,
        model: config.model.clone(),
        provider: "anthropic".into(),
        usage,
        timestamp: now_ms(),
        error_message: None,
        response_id,
    };

    let _ = tx.send(StreamEvent::Done {
        message: message.clone(),
    });

    if let Some(error) = incomplete_tool_use_error {
        Ok(StreamOutcome::IncompleteToolUse {
            assistant: message,
            error,
        })
    } else {
        Ok(StreamOutcome::Complete(message))
    }
}

/// Process a single SSE event. Returns `true` when the stream should stop
/// because Anthropic sent `message_stop` or a recoverable incomplete tool_use
/// error was converted into [`StreamOutcome::IncompleteToolUse`].
fn process_sse_event(
    sse: &SseEvent,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    content: &mut Vec<Content>,
    usage: &mut Usage,
    stop_reason: &mut StopReason,
    response_id: &mut Option<String>,
    incomplete_tool_use_error: &mut Option<ProviderError>,
) -> Result<bool, ProviderError> {
    match sse.event.as_str() {
        "message_start" => {
            if let Ok(data) = serde_json::from_str::<AnthropicMessageStart>(&sse.data) {
                usage.input = data.message.usage.input_tokens;
                usage.cache_read = data.message.usage.cache_read_input_tokens;
                usage.cache_write = data.message.usage.cache_creation_input_tokens;
                if let Some(id) = data.message.id {
                    if !id.is_empty() {
                        *response_id = Some(id);
                    }
                }
            }
        }
        "content_block_start" => {
            if let Ok(data) = serde_json::from_str::<AnthropicContentBlockStart>(&sse.data) {
                let idx = data.index as usize;
                match data.content_block {
                    AnthropicContentBlock::Text { .. } => {
                        while content.len() <= idx {
                            content.push(Content::Text {
                                text: String::new(),
                            });
                        }
                    }
                    AnthropicContentBlock::Thinking { .. } => {
                        while content.len() <= idx {
                            content.push(Content::Thinking {
                                thinking: String::new(),
                                signature: None,
                            });
                        }
                    }
                    AnthropicContentBlock::ToolUse { id, name, .. } => {
                        while content.len() <= idx {
                            content.push(Content::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: serde_json::Value::Object(Default::default()),
                            });
                        }
                        let _ = tx.send(StreamEvent::ToolCallStart {
                            content_index: idx,
                            id,
                            name,
                        });
                    }
                }
            }
        }
        "content_block_delta" => {
            if let Ok(data) = serde_json::from_str::<AnthropicContentBlockDelta>(&sse.data) {
                let idx = data.index as usize;
                match data.delta {
                    AnthropicDelta::TextDelta { text } => {
                        if let Some(Content::Text { text: ref mut t }) = content.get_mut(idx) {
                            t.push_str(&text);
                        }
                        let _ = tx.send(StreamEvent::TextDelta {
                            content_index: idx,
                            delta: text,
                        });
                    }
                    AnthropicDelta::ThinkingDelta { thinking } => {
                        if let Some(Content::Thinking {
                            thinking: ref mut t,
                            ..
                        }) = content.get_mut(idx)
                        {
                            t.push_str(&thinking);
                        }
                        let _ = tx.send(StreamEvent::ThinkingDelta {
                            content_index: idx,
                            delta: thinking,
                        });
                    }
                    AnthropicDelta::InputJsonDelta { partial_json } => {
                        if let Some(Content::ToolCall {
                            ref mut arguments, ..
                        }) = content.get_mut(idx)
                        {
                            let buf = arguments
                                .as_object_mut()
                                .and_then(|o| o.get_mut("__partial_json"))
                                .and_then(|v| v.as_str().map(|s| s.to_string()));
                            let new_buf = format!("{}{}", buf.unwrap_or_default(), partial_json);
                            if let Some(obj) = arguments.as_object_mut() {
                                obj.insert(
                                    "__partial_json".into(),
                                    serde_json::Value::String(new_buf),
                                );
                            }
                        }
                        let _ = tx.send(StreamEvent::ToolCallDelta {
                            content_index: idx,
                            delta: partial_json,
                        });
                    }
                    AnthropicDelta::SignatureDelta { signature } => {
                        if let Some(Content::Thinking {
                            signature: ref mut s,
                            ..
                        }) = content.get_mut(idx)
                        {
                            *s = Some(signature);
                        }
                    }
                }
            }
        }
        "content_block_stop" => {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&sse.data) {
                let idx = data["index"].as_u64().unwrap_or(0) as usize;
                if let Some(Content::ToolCall { arguments, .. }) = content.get_mut(idx) {
                    finalize_tool_call_input(arguments);
                }
                let _ = tx.send(StreamEvent::ToolCallEnd { content_index: idx });
            }
        }
        "message_delta" => {
            if let Ok(data) = serde_json::from_str::<AnthropicMessageDelta>(&sse.data) {
                *stop_reason = match data.delta.stop_reason.as_deref() {
                    Some("tool_use") => StopReason::ToolUse,
                    Some("max_tokens") => StopReason::Length,
                    _ => StopReason::Stop,
                };
                usage.output = data.usage.output_tokens;
            }
        }
        "message_stop" => {
            return Ok(true);
        }
        "ping" | "message" => {}
        "error" => {
            let provider_err = classify_sse_error_event(&sse.data);
            if has_partial_tool_use_input(content) {
                debug!("Anthropic SSE error after partial tool_use input; returning incomplete tool_use outcome for conversation recovery: {provider_err}");
                *incomplete_tool_use_error = Some(provider_err);
                return Ok(true);
            }
            return Err(provider_err);
        }
        other => {
            debug!("Unknown Anthropic event: {}", other);
        }
    }
    Ok(false)
}

fn finalize_tool_call_inputs(content: &mut [Content]) {
    for block in content.iter_mut() {
        let Content::ToolCall { arguments, .. } = block else {
            continue;
        };
        finalize_tool_call_input(arguments);
    }
}

fn finalize_tool_call_input(arguments: &mut serde_json::Value) {
    let Some(partial) = arguments
        .as_object()
        .and_then(|o| o.get("__partial_json"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    else {
        return;
    };

    match crate::provider::json_repair::try_repair_json(&partial) {
        Ok(parsed) => *arguments = parsed,
        Err(e) => {
            debug!("Failed to parse tool call JSON: {} ({})", partial, e);
            *arguments = serde_json::Value::Object(Default::default());
        }
    }
}

fn has_partial_tool_use_input(content: &[Content]) -> bool {
    content.iter().any(|c| {
        let Content::ToolCall { arguments, .. } = c else {
            return false;
        };
        let Some(obj) = arguments.as_object() else {
            return true;
        };
        obj.iter().any(|(key, value)| {
            if key == "__partial_json" {
                return value.as_str().map(|s| !s.is_empty()).unwrap_or(false);
            }
            true
        })
    })
}
