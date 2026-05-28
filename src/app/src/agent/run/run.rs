//! Run — a single execution handle (event stream + control plane).
//!
//! `Run` is a one-shot consumer: call `next()` to read events.
//! For control (steer / follow_up / abort), use `handle()` to get
//! a cloneable `RunControl`.
//!
//! Internally a Run may span multiple engine turns (auto-continuation
//! while the run is active). `RunControl` survives engine swaps
//! between turns; consumers see a single stable handle.

use tokio::sync::mpsc;

use super::control::RunControl;
use super::event::RunEvent;

/// A single agent run. Owns the event stream and a control handle.
///
/// Not cloneable — use `handle()` to obtain a shareable `RunControl`.
pub struct Run {
    pub id: String,
    pub session_id: String,
    rx: mpsc::UnboundedReceiver<RunEvent>,
    control: RunControl,
}

impl Run {
    pub(crate) fn new(
        id: String,
        session_id: String,
        rx: mpsc::UnboundedReceiver<RunEvent>,
        control: RunControl,
    ) -> Self {
        Self {
            id,
            session_id,
            rx,
            control,
        }
    }

    /// Read the next event. Returns `None` when the run is finished.
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.rx.recv().await
    }

    /// Get a cloneable control handle for this run.
    pub fn handle(&self) -> RunControl {
        self.control.clone()
    }

    /// Abort this run.
    pub fn abort(&self) {
        self.control.abort();
    }

    /// Test-only constructor: create a Run from a raw receiver.
    #[doc(hidden)]
    pub fn from_receiver(
        rx: mpsc::UnboundedReceiver<RunEvent>,
        session_id: String,
        run_id: String,
    ) -> Self {
        Self::new(run_id, session_id, rx, RunControl::new())
    }
}
