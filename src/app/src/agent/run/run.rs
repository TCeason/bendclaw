//! Run — a single execution handle (event stream + control plane).
//!
//! `Run` is a one-shot consumer: call `next()` to read events.
//! For control (steer / follow_up / abort), use `handle()` to get
//! a cloneable `RunHandle`.

use tokio::sync::mpsc;

use super::event::RunEvent;

/// A single agent run. Owns the event stream and a control handle.
///
/// Not cloneable — use `handle()` to obtain a shareable `RunHandle`.
pub struct Run {
    pub id: String,
    pub session_id: String,
    rx: mpsc::UnboundedReceiver<RunEvent>,
    handle: evot_engine::RunHandle,
}

impl Run {
    pub(crate) fn new(
        id: String,
        session_id: String,
        rx: mpsc::UnboundedReceiver<RunEvent>,
        handle: evot_engine::RunHandle,
    ) -> Self {
        Self {
            id,
            session_id,
            rx,
            handle,
        }
    }

    /// Read the next event. Returns `None` when the run is finished.
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.rx.recv().await
    }

    /// Get a cloneable control handle for this run.
    pub fn handle(&self) -> evot_engine::RunHandle {
        self.handle.clone()
    }

    /// Abort this run.
    pub fn abort(&self) {
        self.handle.abort();
    }

    /// Test-only constructor: create a Run from a raw receiver.
    #[doc(hidden)]
    pub fn from_receiver(
        rx: mpsc::UnboundedReceiver<RunEvent>,
        session_id: String,
        run_id: String,
    ) -> Self {
        Self::new(run_id, session_id, rx, evot_engine::RunHandle::noop())
    }
}
