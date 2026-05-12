use crate::context::compaction::policy::CompactionPolicy;
use crate::types::AgentMessage;

/// Shared read-only context passed to every phase transform.
pub struct PhaseContext {
    pub budget: BudgetTargets,
    pub bounds: RetentionBounds,
    pub shrink: ShrinkSettings,
    pub policy: CompactionPolicy,
}

pub struct BudgetTargets {
    pub max_tokens: usize,
    pub compact_target: usize,
}

pub struct RetentionBounds {
    pub keep_recent: usize,
    pub keep_first: usize,
    pub max_messages: usize,
    pub message_limit_target_pct: u8,
}

pub struct ShrinkSettings {
    pub tool_output_max_lines: usize,
}

pub struct PhaseResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<super::CompactionAction>,
}
