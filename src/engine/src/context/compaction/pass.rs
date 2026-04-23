//! Shared types for compaction passes.

use super::compact::CompactionAction;
use super::policy::CompactionPolicy;
use crate::types::AgentMessage;

/// Shared read-only context passed to every pass.
pub struct CompactContext {
    pub budget: usize,
    /// L1 collapse target: trigger and stop threshold for turn summarization.
    /// Set below `budget` so L1 runs before hitting the hard limit and
    /// compresses enough to avoid per-turn "toothpaste squeezing".
    pub l1_target: usize,
    pub keep_recent: usize,
    pub keep_first: usize,
    pub tool_output_max_lines: usize,
    pub policy: CompactionPolicy,
}

/// Output of a single pass.
pub struct PassResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<CompactionAction>,
}
