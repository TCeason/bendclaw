use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use evot_engine::host::HostToolResponse;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::convert::parse_content_blocks;
use crate::host::HostResponders;

/// Serialize a RunEvent to JSON string.
fn serialize_event(event: evot::agent::RunEvent) -> Result<Option<String>> {
    serde_json::to_string(&event)
        .map(Some)
        .map_err(|e| Error::from_reason(format!("serialize event: {e}")))
}

/// Wire shape of a host tool response from JS: the correlation id plus the
/// engine's [`HostToolResponse`] fields, flattened.
#[derive(serde::Deserialize)]
struct HostToolResponsePayload {
    tool_call_id: String,
    #[serde(flatten)]
    response: HostToolResponse,
}

/// Abort-aware helper: read next event from run, or return None on abort.
macro_rules! next_run_or_abort {
    ($run:expr, $notify:expr) => {
        tokio::select! {
            event = $run.next() => {
                match event {
                    Some(e) => serialize_event(e),
                    None => Ok(None),
                }
            }
            _ = $notify.notified() => {
                $run.abort();
                Ok(None)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// NapiSubmitOutcome
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiSubmitOutcome {
    pub(crate) kind: String,
    pub(crate) run: std::sync::Mutex<Option<NapiRun>>,
    pub(crate) message: Option<String>,
}

#[napi]
impl NapiSubmitOutcome {
    #[napi(getter)]
    pub fn kind(&self) -> String {
        self.kind.clone()
    }

    #[napi(getter)]
    pub fn message(&self) -> Option<String> {
        self.message.clone()
    }

    #[napi]
    pub fn take_run(&self) -> Option<NapiRun> {
        if let Ok(mut guard) = self.run.lock() {
            guard.take()
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// NapiRun — streaming event iterator for a single agent run
// ---------------------------------------------------------------------------

#[napi]
pub struct NapiRun {
    pub(crate) inner: Mutex<evot::agent::Run>,
    pub(crate) handle: evot::agent::RunControl,
    pub(crate) cached_session_id: String,
    pub(crate) aborted: Arc<AtomicBool>,
    pub(crate) abort_notify: Arc<Notify>,
    pub(crate) host_event_rx: Mutex<Option<tokio_mpsc::UnboundedReceiver<String>>>,
    pub(crate) host_responders: HostResponders,
}

#[napi]
impl NapiRun {
    #[napi(getter)]
    pub fn session_id(&self) -> String {
        self.cached_session_id.clone()
    }

    /// Get the next event from the run stream.
    /// Returns null when the run is complete.
    #[napi]
    pub async fn next(&self) -> Result<Option<String>> {
        if self.aborted.load(Ordering::Relaxed) {
            return Ok(None);
        }

        let mut run = self.inner.lock().await;

        // Check if we have a host-event receiver (host tool call forwarding).
        let mut host_rx_guard = self.host_event_rx.lock().await;
        let host_rx_slot = &mut *host_rx_guard;

        match host_rx_slot {
            None => next_run_or_abort!(run, self.abort_notify),
            Some(host_rx) => {
                tokio::select! {
                    host_json = host_rx.recv() => {
                        match host_json {
                            Some(json) => Ok(Some(json)),
                            None => {
                                // Sender dropped and buffer empty — permanently
                                // disable the host branch, then read from run.
                                *host_rx_slot = None;
                                next_run_or_abort!(run, self.abort_notify)
                            }
                        }
                    }
                    event = run.next() => {
                        match event {
                            Some(e) => serialize_event(e),
                            None => Ok(None),
                        }
                    }
                    _ = self.abort_notify.notified() => {
                        run.abort();
                        Ok(None)
                    }
                }
            }
        }
    }

    /// Respond to a `host_tool_call` event with a JSON-encoded result.
    ///
    /// The payload is `{ tool_call_id, content, details?, is_error? }`. The
    /// call is matched to its parked responder by `tool_call_id`, so parallel
    /// host tool calls resolve independently.
    #[napi]
    pub async fn respond_host_tool(&self, response_json: String) -> Result<()> {
        let parsed: HostToolResponsePayload = serde_json::from_str(&response_json)
            .map_err(|e| Error::from_reason(format!("parse host tool response: {e}")))?;
        let mut guard = self.host_responders.lock().await;
        if let Some(tx) = guard.remove(&parsed.tool_call_id) {
            let _ = tx.send(Ok(parsed.response));
        }
        Ok(())
    }

    /// Abort the running query. Safe to call while next() is awaiting.
    #[napi]
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
        self.abort_notify.notify_waiters();
        self.handle.abort();
    }

    /// Send a steering message into the running agent loop.
    #[napi]
    pub fn steer(&self, text: String, content_json: Option<String>) {
        let content = if let Some(json) = content_json {
            if let Ok(blocks) = parse_content_blocks(&json) {
                if blocks.is_empty() {
                    vec![evot_engine::Content::Text { text }]
                } else {
                    blocks
                }
            } else {
                vec![evot_engine::Content::Text { text }]
            }
        } else {
            vec![evot_engine::Content::Text { text }]
        };
        self.handle
            .steer(evot_engine::AgentMessage::Llm(evot_engine::Message::User {
                content,
                timestamp: evot_engine::now_ms(),
            }));
    }

    /// Send a follow-up message (processed after current turn finishes).
    #[napi]
    pub fn follow_up(&self, text: String) {
        self.handle
            .follow_up(evot_engine::AgentMessage::Llm(evot_engine::Message::user(
                text,
            )));
    }
}
