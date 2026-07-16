//! RunHandle — cloneable control handle for a single run.

use tokio_util::sync::CancellationToken;

use super::PromptQueue;
use super::PromptQueueEntry;
use super::PromptQueueError;
use crate::types::*;

/// Cloneable control handle for a running agent loop.
///
/// `Run` (app layer) owns the event stream; `RunHandle` is the control plane.
/// Clone it freely to steer / follow-up / abort from any thread.
#[derive(Clone)]
pub struct RunHandle {
    pub(super) steering_queue: PromptQueue,
    pub(super) follow_up_queue: PromptQueue,
    pub(super) cancel: CancellationToken,
}

impl RunHandle {
    /// Queue a steering message (interrupts agent mid-tool-execution).
    pub fn steer(&self, msg: AgentMessage) -> PromptQueueEntry {
        self.steering_queue.enqueue(msg)
    }

    /// Queue a follow-up message (processed after agent finishes current turn).
    pub fn follow_up(&self, msg: AgentMessage) -> PromptQueueEntry {
        self.follow_up_queue.enqueue(msg)
    }

    pub fn steering_entries(&self) -> Vec<PromptQueueEntry> {
        self.steering_queue.list()
    }

    pub fn follow_up_entries(&self) -> Vec<PromptQueueEntry> {
        self.follow_up_queue.list()
    }

    pub fn update_steering(
        &self,
        id: &str,
        expected_version: u64,
        message: AgentMessage,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.steering_queue.update(id, expected_version, message)
    }

    pub fn update_follow_up(
        &self,
        id: &str,
        expected_version: u64,
        message: AgentMessage,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.follow_up_queue.update(id, expected_version, message)
    }

    pub fn remove_steering(
        &self,
        id: &str,
        expected_version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.steering_queue.remove(id, expected_version)
    }

    pub fn remove_follow_up(
        &self,
        id: &str,
        expected_version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.follow_up_queue.remove(id, expected_version)
    }

    /// Promote a queued follow-up to steering so it is delivered at the next
    /// interruption point instead of waiting for the current turn to finish.
    pub fn send_follow_up_now(
        &self,
        id: &str,
        expected_version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let entry = self.follow_up_queue.remove(id, expected_version)?;
        Ok(self.steering_queue.push_existing(entry))
    }

    /// Clear all queued steering messages.
    pub fn clear_steering(&self) {
        self.steering_queue.clear();
    }

    /// Clear all queued follow-up messages.
    pub fn clear_follow_up(&self) {
        self.follow_up_queue.clear();
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
            steering_queue: PromptQueue::new(),
            follow_up_queue: PromptQueue::new(),
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
