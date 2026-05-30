//! RunControl — app-level control plane for a Run.
//!
//! Lives across multiple internal engine turns (auto-continuation), so
//! callers see a single, stable handle even when the runtime swaps the
//! underlying engine `RunHandle` between turns.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// Cloneable control handle for a Run from the app/CLI side.
///
/// Forwards `abort` to the current engine handle and to the run's own
/// cancellation token. Steering / follow-up forward to the engine handle
/// when one is set; if no engine is currently active (between auto-
/// continuation turns), the call is a no-op.
#[derive(Clone)]
pub struct RunControl {
    cancel: CancellationToken,
    engine: Arc<Mutex<Option<evot_engine::RunHandle>>>,
}

impl RunControl {
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
            engine: Arc::new(Mutex::new(None)),
        }
    }

    /// Swap in the engine handle for the next turn. Called by the runtime
    /// at the start of every internal turn.
    pub(in crate::agent) fn install_engine(&self, handle: evot_engine::RunHandle) {
        *self.engine.lock() = Some(handle);
    }

    /// Drop the current engine handle (e.g. between turns). The control
    /// remains valid; abort still cancels via the run-level token.
    pub(in crate::agent) fn detach_engine(&self) {
        *self.engine.lock() = None;
    }

    /// Abort the run. Cancels the current engine turn (if any) and marks
    /// the run as cancelled so no further auto-continuation is scheduled.
    pub fn abort(&self) {
        self.cancel.cancel();
        if let Some(h) = self.engine.lock().as_ref() {
            h.abort();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub(in crate::agent) fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Forward a steering message to the active engine turn, if any.
    pub fn steer(&self, msg: evot_engine::AgentMessage) {
        if let Some(h) = self.engine.lock().as_ref() {
            h.steer(msg);
        }
    }

    /// Forward a follow-up message to the active engine turn, if any.
    pub fn follow_up(&self, msg: evot_engine::AgentMessage) {
        if let Some(h) = self.engine.lock().as_ref() {
            h.follow_up(msg);
        }
    }
}

impl Default for RunControl {
    fn default() -> Self {
        Self::new()
    }
}
