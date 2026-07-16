//! Run-level prompt queues that survive per-turn engine replacement.

use evot_engine::PromptQueue;
use evot_engine::PromptQueueEntry;
use evot_engine::PromptQueueError;

#[derive(Clone, Default)]
pub struct RunQueues {
    steering: PromptQueue,
    follow_up: PromptQueue,
}

impl RunQueues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn steering(&self) -> PromptQueue {
        self.steering.clone()
    }

    pub fn follow_up(&self) -> PromptQueue {
        self.follow_up.clone()
    }

    pub fn enqueue_steering(&self, message: evot_engine::AgentMessage) -> PromptQueueEntry {
        self.steering.enqueue(message)
    }

    pub fn enqueue_follow_up(&self, message: evot_engine::AgentMessage) -> PromptQueueEntry {
        self.follow_up.enqueue(message)
    }

    pub fn list_steering(&self) -> Vec<PromptQueueEntry> {
        self.steering.list()
    }

    pub fn list_follow_up(&self) -> Vec<PromptQueueEntry> {
        self.follow_up.list()
    }

    pub fn update_steering(
        &self,
        id: &str,
        version: u64,
        message: evot_engine::AgentMessage,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.steering.update(id, version, message)
    }

    pub fn update_follow_up(
        &self,
        id: &str,
        version: u64,
        message: evot_engine::AgentMessage,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.follow_up.update(id, version, message)
    }

    pub fn remove_steering(
        &self,
        id: &str,
        version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.steering.remove(id, version)
    }

    pub fn remove_follow_up(
        &self,
        id: &str,
        version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.follow_up.remove(id, version)
    }

    pub fn move_steering_up(
        &self,
        id: &str,
        version: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.steering.move_up(id, version)
    }

    pub fn move_steering_down(
        &self,
        id: &str,
        version: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.steering.move_down(id, version)
    }

    pub fn move_follow_up_up(
        &self,
        id: &str,
        version: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.follow_up.move_up(id, version)
    }

    pub fn move_follow_up_down(
        &self,
        id: &str,
        version: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.follow_up.move_down(id, version)
    }

    pub fn send_follow_up_now(
        &self,
        id: &str,
        version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let entry = self.follow_up.remove(id, version)?;
        Ok(self.steering.push_existing(entry))
    }

    pub fn clear_steering(&self) {
        self.steering.clear();
    }

    pub fn clear_follow_up(&self) {
        self.follow_up.clear();
    }
}
