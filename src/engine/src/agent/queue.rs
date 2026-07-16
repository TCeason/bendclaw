//! Stable, editable prompt queues shared by the agent and its run handles.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;

use crate::types::AgentMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDrainMode {
    One,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptQueueEntry {
    pub id: String,
    pub version: u64,
    pub message: AgentMessage,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PromptQueueError {
    #[error("queued prompt not found: {0}")]
    NotFound(String),
    #[error("queued prompt version conflict: expected {expected}, actual {actual}")]
    VersionConflict { expected: u64, actual: u64 },
}

#[derive(Clone, Default)]
pub struct PromptQueue {
    entries: Arc<Mutex<VecDeque<PromptQueueEntry>>>,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&self, message: AgentMessage) -> PromptQueueEntry {
        let entry = PromptQueueEntry {
            id: uuid::Uuid::new_v4().to_string(),
            version: 0,
            message,
        };
        self.entries.lock().push_back(entry.clone());
        entry
    }

    pub fn list(&self) -> Vec<PromptQueueEntry> {
        self.entries.lock().iter().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
    }

    pub fn update(
        &self,
        id: &str,
        expected_version: u64,
        message: AgentMessage,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let mut entries = self.entries.lock();
        let entry = entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| PromptQueueError::NotFound(id.to_string()))?;
        if entry.version != expected_version {
            return Err(PromptQueueError::VersionConflict {
                expected: expected_version,
                actual: entry.version,
            });
        }
        entry.version = entry.version.saturating_add(1);
        entry.message = message;
        Ok(entry.clone())
    }

    pub fn remove(
        &self,
        id: &str,
        expected_version: Option<u64>,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let mut entries = self.entries.lock();
        let index = entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| PromptQueueError::NotFound(id.to_string()))?;
        if let Some(expected) = expected_version {
            let actual = entries[index].version;
            if actual != expected {
                return Err(PromptQueueError::VersionConflict { expected, actual });
            }
        }
        entries
            .remove(index)
            .ok_or_else(|| PromptQueueError::NotFound(id.to_string()))
    }

    pub fn move_up(
        &self,
        id: &str,
        expected_version: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.move_by(id, expected_version, -1)
    }

    pub fn move_down(
        &self,
        id: &str,
        expected_version: u64,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        self.move_by(id, expected_version, 1)
    }

    fn move_by(
        &self,
        id: &str,
        expected_version: u64,
        offset: isize,
    ) -> Result<PromptQueueEntry, PromptQueueError> {
        let mut entries = self.entries.lock();
        let index = entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| PromptQueueError::NotFound(id.to_string()))?;
        let actual = entries[index].version;
        if actual != expected_version {
            return Err(PromptQueueError::VersionConflict {
                expected: expected_version,
                actual,
            });
        }
        let target = index
            .saturating_add_signed(offset)
            .min(entries.len().saturating_sub(1));
        if target != index {
            entries.swap(index, target);
            entries[index].version = entries[index].version.saturating_add(1);
            entries[target].version = entries[target].version.saturating_add(1);
        }
        Ok(entries[target].clone())
    }

    pub fn push_existing(&self, mut entry: PromptQueueEntry) -> PromptQueueEntry {
        entry.version = entry.version.saturating_add(1);
        self.entries.lock().push_back(entry.clone());
        entry
    }

    pub fn drain_messages(&self, mode: QueueDrainMode) -> Vec<AgentMessage> {
        let mut entries = self.entries.lock();
        match mode {
            QueueDrainMode::One => entries
                .pop_front()
                .map(|entry| vec![entry.message])
                .unwrap_or_default(),
            QueueDrainMode::All => entries.drain(..).map(|entry| entry.message).collect(),
        }
    }
}
