//! Anthropic SSE stream decoding.
//!
//! Parses Anthropic Messages API SSE events and translates them into
//! internal [`StreamEvent`]s while accumulating the final [`Message`].

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::warn;

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

    let mut state = AnthropicSseState::default();

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
                        if process_sse_event(&sse, &tx, &mut state)? {
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
        debug!(
            "SSE driver error (content_len={}): {e}",
            state.content.len()
        );
        return Err(ProviderError::Network(e));
    }

    // Match pi/Anthropic semantics: once a message_start was observed, the
    // stream is only complete after message_stop. A clean socket EOF without
    // message_stop is still an interrupted stream; using partial content would
    // persist half-written assistant markdown as a normal `stop` response.
    if state.saw_message_start && !state.saw_message_stop {
        return Err(ProviderError::Network(
            "Anthropic stream ended before message_stop".into(),
        ));
    }

    // Detect empty response: no content and no usage from provider
    if state.content.is_empty() && state.usage.input == 0 && state.usage.output == 0 {
        return Err(ProviderError::Api(
            "Empty response from provider (no content, no usage)".into(),
        ));
    }

    let has_tool_calls = state
        .content
        .iter()
        .any(|c| matches!(c, Content::ToolCall { .. }));
    // Never mask a provider-terminal error (e.g. refusal) as a normal
    // tool_use stop — the error must surface to the caller.
    if has_tool_calls && state.stop_reason != StopReason::Error {
        state.stop_reason = StopReason::ToolUse;
    }

    let message = Message::Assistant {
        content: state.content,
        stop_reason: state.stop_reason,
        // Report the model that actually served the request. On server-side
        // fallback (e.g. claude-fable-5 → claude-opus-4-8) this is the
        // substitute model from the `fallback` block, so the TUI shows what
        // really happened instead of the requested model.
        model: state.fallback_model.unwrap_or_else(|| config.model.clone()),
        provider: "anthropic".into(),
        usage: state.usage,
        timestamp: now_ms(),
        error_message: state.error_message,
        response_id: state.response_id,
    };

    let _ = tx.send(StreamEvent::Done {
        message: message.clone(),
    });

    Ok(StreamOutcome::from(message))
}

struct AnthropicSseState {
    content: Vec<Content>,
    tool_input_buffers: HashMap<usize, String>,
    usage: Usage,
    stop_reason: StopReason,
    response_id: Option<String>,
    saw_message_start: bool,
    saw_message_stop: bool,
    /// Human-readable reason when the stream terminated with
    /// `StopReason::Error` (e.g. a safety refusal), so the caller sees the
    /// actual cause instead of a generic "Unknown error".
    error_message: Option<String>,
    /// Substitute model from a server-side `fallback` block, if any.
    fallback_model: Option<String>,
}

impl Default for AnthropicSseState {
    fn default() -> Self {
        Self {
            content: Vec::new(),
            tool_input_buffers: HashMap::new(),
            usage: Usage::default(),
            stop_reason: StopReason::Stop,
            response_id: None,
            saw_message_start: false,
            saw_message_stop: false,
            error_message: None,
            fallback_model: None,
        }
    }
}

fn parse_event_data<T: DeserializeOwned>(sse: &SseEvent) -> Result<T, ProviderError> {
    serde_json::from_str::<T>(&sse.data).map_err(|e| {
        ProviderError::Api(format!(
            "Could not parse Anthropic SSE event {}: {}; data={}",
            sse.event, e, sse.data
        ))
    })
}

fn map_stop_reason(reason: &str) -> Result<StopReason, ProviderError> {
    match reason {
        "end_turn" => Ok(StopReason::Stop),
        "max_tokens" => Ok(StopReason::Length),
        "tool_use" => Ok(StopReason::ToolUse),
        // Mirrors pi: pause_turn/stop_sequence are safe stop-like terminal
        // reasons here. We do not configure stop sequences, so seeing one is
        // unusual but still provider-terminal rather than stream-corrupt.
        "pause_turn" | "stop_sequence" => Ok(StopReason::Stop),
        // Safety/refusal terminal reasons should not be silently reported as a
        // successful stop.
        "refusal" | "sensitive" => Ok(StopReason::Error),
        other => Err(ProviderError::Api(format!(
            "Unhandled Anthropic stop reason: {other}"
        ))),
    }
}

fn process_sse_event(
    sse: &SseEvent,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    state: &mut AnthropicSseState,
) -> Result<bool, ProviderError> {
    match sse.event.as_str() {
        "message_start" => {
            state.saw_message_start = true;
            let data = parse_event_data::<AnthropicMessageStart>(sse)?;
            state.usage.input = data.message.usage.input_tokens;
            state.usage.cache_read = data.message.usage.cache_read_input_tokens;
            state.usage.cache_write = data.message.usage.cache_creation_input_tokens;
            if let Some(id) = data.message.id {
                if !id.is_empty() {
                    state.response_id = Some(id);
                }
            }
        }
        "content_block_start" => {
            let data = parse_event_data::<AnthropicContentBlockStart>(sse)?;
            let idx = data.index as usize;
            // Fill index gaps (from unknown blocks) with empty text
            // placeholders — never clones of the current block, which would
            // duplicate tool_use ids and get every later request rejected.
            while state.content.len() < idx {
                state.content.push(Content::Text {
                    text: String::new(),
                });
            }
            match data.content_block {
                AnthropicContentBlock::Text { .. } => {
                    state.content.push(Content::Text {
                        text: String::new(),
                    });
                }
                AnthropicContentBlock::Thinking { .. } => {
                    state.content.push(Content::Thinking {
                        thinking: String::new(),
                        signature: None,
                    });
                }
                AnthropicContentBlock::ToolUse { id, name, .. } => {
                    state.content.push(Content::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: serde_json::Value::Object(Default::default()),
                    });
                    let _ = tx.send(StreamEvent::ToolCallStart {
                        content_index: idx,
                        id,
                        name,
                    });
                }
                // Server-side model fallback (e.g. fable-5 → opus-4-8). Record
                // the substitute model so the response reports what actually
                // served the request, and keep a placeholder for alignment.
                AnthropicContentBlock::Fallback { to } => {
                    if let Some(model) = to.and_then(|t| t.model) {
                        if !model.is_empty() {
                            state.fallback_model = Some(model);
                        }
                    }
                    state.content.push(Content::Text {
                        text: String::new(),
                    });
                }
                // Unknown block type: keep a placeholder so later indices stay
                // aligned.
                AnthropicContentBlock::Other => {
                    state.content.push(Content::Text {
                        text: String::new(),
                    });
                }
            }
        }
        "content_block_delta" => {
            let data = parse_event_data::<AnthropicContentBlockDelta>(sse)?;
            let idx = data.index as usize;
            match data.delta {
                AnthropicDelta::TextDelta { text } => {
                    if let Some(Content::Text { text: ref mut t }) = state.content.get_mut(idx) {
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
                    }) = state.content.get_mut(idx)
                    {
                        t.push_str(&thinking);
                    }
                    let _ = tx.send(StreamEvent::ThinkingDelta {
                        content_index: idx,
                        delta: thinking,
                    });
                }
                AnthropicDelta::InputJsonDelta { partial_json } => {
                    state
                        .tool_input_buffers
                        .entry(idx)
                        .or_default()
                        .push_str(&partial_json);
                    let _ = tx.send(StreamEvent::ToolCallDelta {
                        content_index: idx,
                        delta: partial_json,
                    });
                }
                AnthropicDelta::SignatureDelta { signature } => {
                    if let Some(Content::Thinking {
                        signature: ref mut s,
                        ..
                    }) = state.content.get_mut(idx)
                    {
                        *s = Some(signature);
                    }
                }
                // Unknown delta type: ignore for forward compatibility.
                AnthropicDelta::Other => {}
            }
        }
        "content_block_stop" => {
            let data = parse_event_data::<serde_json::Value>(sse)?;
            let idx = data["index"].as_u64().unwrap_or(0) as usize;
            if let Some(input) = state.tool_input_buffers.remove(&idx) {
                if let Some(Content::ToolCall { arguments, .. }) = state.content.get_mut(idx) {
                    match serde_json::from_str(&input) {
                        Ok(parsed) => *arguments = parsed,
                        Err(e) => debug!("Failed to parse tool call JSON: {} ({})", input, e),
                    }
                }
            }
            let _ = tx.send(StreamEvent::ToolCallEnd { content_index: idx });
        }
        "message_delta" => {
            let data = parse_event_data::<AnthropicMessageDelta>(sse)?;
            if let Some(reason) = data.delta.stop_reason.as_deref() {
                state.stop_reason = map_stop_reason(reason)?;
                if state.stop_reason == StopReason::Error {
                    state.error_message = Some(format!(
                        "Provider ended the response with stop reason '{reason}' (safety filter / refusal)"
                    ));
                }
            }
            state.usage.output = data.usage.output_tokens;
            // Only override cache fields when the delta actually carries
            // them — Anthropic's SSE spec only guarantees `output_tokens`
            // in `message_delta.usage`, so a missing field (decoded as 0)
            // must not clobber values captured from `message_start`.
            if data.usage.cache_read_input_tokens > 0 {
                state.usage.cache_read = data.usage.cache_read_input_tokens;
            }
            if data.usage.cache_creation_input_tokens > 0 {
                state.usage.cache_write = data.usage.cache_creation_input_tokens;
            }
        }
        "message_stop" => {
            let _ = parse_event_data::<serde_json::Value>(sse)?;
            state.saw_message_stop = true;
            return Ok(true);
        }
        "ping" | "message" => {}
        "error" => {
            // Surface the exact malformed model output when the gateway rejects a
            // tool call. Two sources, in priority order:
            //   1. The `raw` field the gateway attaches to the error payload —
            //      the model's original text when it couldn't assemble a valid
            //      tool_use block at all (nothing was streamed to us).
            //   2. Any tool-call JSON we buffered from streamed deltas before the
            //      error arrived.
            // Either way this is our only window into the bad format, since the
            // error path discards the partial content.
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&sse.data) {
                if let Some(raw) = payload
                    .get("error")
                    .and_then(|e| e.get("raw"))
                    .and_then(|r| r.as_str())
                {
                    warn!(
                        raw_tool_json = raw,
                        "provider error with raw tool-call JSON from gateway"
                    );
                }
            }
            if !state.tool_input_buffers.is_empty() {
                for (idx, raw) in &state.tool_input_buffers {
                    warn!(
                        block_index = idx,
                        raw_tool_json = raw.as_str(),
                        "provider error with buffered tool-call JSON"
                    );
                }
            }
            return Err(classify_sse_error_event(&sse.data));
        }
        other => {
            debug!("Unknown Anthropic event: {}", other);
        }
    }
    Ok(false)
}
