use std::convert::Infallible;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::BendclawError;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::EventSink;
use crate::request::RequestFinishedPayload;
use crate::request::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

pub type SseEvent = std::result::Result<axum::response::sse::Event, Infallible>;

pub(crate) struct SseSink {
    tx: tokio::sync::mpsc::Sender<SseEvent>,
}

impl SseSink {
    pub(crate) fn new(tx: tokio::sync::mpsc::Sender<SseEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EventSink for SseSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        for item in map_run_event(event.as_ref()) {
            self.tx
                .send(item)
                .await
                .map_err(|e| BendclawError::Run(format!("failed to publish server event: {e}")))?;
        }
        Ok(())
    }
}

pub fn done_event() -> SseEvent {
    event("done", &json!(null))
}

pub fn error_event(message: impl Into<String>) -> SseEvent {
    event("error", &json!({ "message": message.into() }))
}

pub fn map_run_event(run_event: &RunEvent) -> Vec<SseEvent> {
    let mut events = Vec::new();

    match &run_event.kind {
        RunEventKind::AssistantCompleted => {
            if let Some(payload) = payload_as::<AssistantPayload>(&run_event.payload) {
                for block in payload.content {
                    match block {
                        AssistantBlock::Text { .. } => {}
                        AssistantBlock::ToolCall { id, name, input } => {
                            events.push(event(
                                "tool_call",
                                &json!({ "id": id, "name": name, "input": input }),
                            ));
                        }
                        AssistantBlock::Thinking { text } if !text.is_empty() => {
                            events.push(event("thinking", &json!({ "thinking": text })));
                        }
                        _ => {}
                    }
                }
            }
        }
        RunEventKind::ToolFinished => {
            if let Some(payload) = payload_as::<ToolResultPayload>(&run_event.payload) {
                events.push(event(
                    "tool_result",
                    &json!({
                        "tool_call_id": payload.tool_call_id,
                        "content": payload.content,
                        "is_error": payload.is_error,
                    }),
                ));
            }
        }
        RunEventKind::RunFinished => {
            if let Some(payload) = payload_as::<RequestFinishedPayload>(&run_event.payload) {
                let input_tokens = payload
                    .usage
                    .get("input")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_default();
                let output_tokens = payload
                    .usage
                    .get("output")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_default();
                events.push(event(
                    "result",
                    &json!({
                        "turn_count": payload.turn_count,
                        "input_tokens": input_tokens,
                        "output_tokens": output_tokens,
                        "duration_ms": payload.duration_ms,
                    }),
                ));
            }
        }
        RunEventKind::Error => {
            if let Some(message) = run_event.payload.get("message").and_then(|v| v.as_str()) {
                events.push(error_event(message));
            }
        }
        RunEventKind::AssistantDelta => {
            if let Some(delta) = run_event.payload.get("delta").and_then(|v| v.as_str()) {
                if !delta.is_empty() {
                    events.push(event("text", &json!({ "text": delta })));
                }
            }
        }
        _ => {}
    }

    events
}

fn event(event_type: &str, data: &serde_json::Value) -> SseEvent {
    let payload = json!({ "type": event_type, "data": data });
    match serde_json::to_string(&payload) {
        Ok(json) => Ok(axum::response::sse::Event::default().data(json)),
        Err(_) => Ok(axum::response::sse::Event::default().data(String::new())),
    }
}
