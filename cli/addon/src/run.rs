use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use evot_engine::tools::AskUserResponse;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

use crate::ask::AskResponder;
use crate::convert::parse_content_blocks;

/// Serialize a RunEvent to JSON string.
fn serialize_event(event: evot::agent::RunEvent) -> Result<Option<String>> {
    serde_json::to_string(&event)
        .map(Some)
        .map_err(|e| Error::from_reason(format!("serialize event: {e}")))
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
    pub(crate) handle: evot_engine::RunHandle,
    pub(crate) cached_session_id: String,
    pub(crate) aborted: Arc<AtomicBool>,
    pub(crate) abort_notify: Arc<Notify>,
    pub(crate) ask_event_rx: Mutex<Option<tokio_mpsc::UnboundedReceiver<String>>>,
    pub(crate) ask_responder: AskResponder,
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

        // Check if we have an ask_event receiver
        let mut ask_rx_guard = self.ask_event_rx.lock().await;
        let ask_rx_slot = &mut *ask_rx_guard;

        match ask_rx_slot {
            None => next_run_or_abort!(run, self.abort_notify),
            Some(ask_rx) => {
                tokio::select! {
                    ask_json = ask_rx.recv() => {
                        match ask_json {
                            Some(json) => Ok(Some(json)),
                            None => {
                                // Sender dropped and buffer empty — permanently
                                // disable the ask branch, then read from run.
                                *ask_rx_slot = None;
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

    /// Respond to an `ask_user` event.
    #[napi]
    pub async fn respond_ask_user(&self, response_json: String) -> Result<()> {
        let response: AskUserResponse = serde_json::from_str(&response_json)
            .map_err(|e| Error::from_reason(format!("parse ask_user response: {e}")))?;
        let mut guard = self.ask_responder.lock().await;
        if let Some(tx) = guard.take() {
            let _ = tx.send(Ok(response));
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
