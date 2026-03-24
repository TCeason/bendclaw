//! Dedicated tool call counter for enforcing per-run max_tool_calls.

/// Tracks executed tool calls against a hard cap.
#[derive(Debug, Clone, Copy)]
pub struct ToolCallLimitTracker {
    limit: u32,
    count: u32,
}

impl ToolCallLimitTracker {
    pub fn new(limit: u32) -> Self {
        Self { limit, count: 0 }
    }

    pub fn increment(&mut self, n: u32) {
        self.count = self.count.saturating_add(n);
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }

    pub fn remaining(&self) -> u32 {
        self.limit.saturating_sub(self.count)
    }

    pub fn is_exceeded(&self) -> bool {
        self.count >= self.limit
    }
}
