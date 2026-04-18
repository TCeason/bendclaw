//! RunHandle — cloneable control handle for a single run.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::types::*;

/// Cloneable control handle for a running agent loop.
///
/// `Run` (app layer) owns the event stream; `RunHandle` is the control plane.
/// Clone it freely to steer / follow-up / abort from any thread.
#[derive(Clone)]
pub struct RunHandle {
    pub(super) steering_queue: Arc<Mutex<Vec<AgentMessage>>>,
    pub(super) follow_up_queue: Arc<Mutex<Vec<AgentMessage>>>,
    pub(super) cancel: CancellationToken,
}

impl RunHandle {
    /// Queue a steering message (interrupts agent mid-tool-execution).
    pub fn steer(&self, msg: AgentMessage) {
        self.steering_queue.lock().push(msg);
    }

    /// Queue a follow-up message (processed after agent finishes current turn).
    pub fn follow_up(&self, msg: AgentMessage) {
        self.follow_up_queue.lock().push(msg);
    }

    /// Clear all queued steering messages.
    pub fn clear_steering(&self) {
        self.steering_queue.lock().clear();
    }

    /// Clear all queued follow-up messages.
    pub fn clear_follow_up(&self) {
        self.follow_up_queue.lock().clear();
    }

    /// Abort the run.
    pub fn abort(&self) {
        self.cancel.cancel();
    }

    /// Check if the run has been aborted.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Create a no-op handle (for tests).
    pub fn noop() -> Self {
        Self {
            steering_queue: Arc::new(Mutex::new(Vec::new())),
            follow_up_queue: Arc::new(Mutex::new(Vec::new())),
            cancel: CancellationToken::new(),
        }
    }
}

/// Queue mode for steering and follow-up messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMode {
    /// Deliver one message per turn
    OneAtATime,
    /// Deliver all queued messages at once
    All,
}
