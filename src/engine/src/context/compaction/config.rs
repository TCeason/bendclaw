//! Centralized compaction configuration.
//!
//! All tunable parameters live here. Passes read from this struct
//! to decide *what* to do — they never hard-code thresholds.

/// All compaction parameters in one place.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    // -- Budget --
    /// Max tokens available for messages (context_window - system_prompt - output_reserve).
    pub budget_tokens: usize,
    /// System prompt tokens (not compactable).
    pub system_prompt_tokens: usize,
    /// Trigger compaction when message tokens exceed this percentage of budget.
    pub compact_trigger_pct: u8,
    /// Target percentage after compaction.
    pub compact_target_pct: u8,

    // -- Retention --
    /// Minimum recent messages to always keep at full detail.
    pub keep_recent: usize,
    /// Minimum first messages to always keep.
    pub keep_first: usize,
    /// Max messages before message-count eviction kicks in.
    pub max_messages: usize,
    /// Target percentage of max_messages after eviction.
    pub message_limit_target_pct: u8,

    // -- Microcompact --
    /// Token budget: keep the most recent compactable tool results whose
    /// cumulative tokens fit within this budget. Older results are cleared.
    pub microcompact_keep_tokens: usize,
    /// Keep this many most-recent images (before recent boundary) at full content.
    pub microcompact_keep_images: usize,

    // -- Shrink --
    /// Max lines per tool output in shrink pass.
    pub tool_output_max_lines: usize,
    /// Absolute token threshold — a single result above this is oversized.
    pub oversize_abs_tokens: usize,
    /// Ratio threshold — a single result above `budget * ratio` is oversized.
    pub oversize_budget_ratio: f64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            budget_tokens: 96_000,
            system_prompt_tokens: 4_000,
            compact_trigger_pct: 80,
            compact_target_pct: 75,
            keep_recent: 10,
            keep_first: 2,
            max_messages: 150,
            message_limit_target_pct: 90,

            microcompact_keep_tokens: 40_000,
            microcompact_keep_images: 1,

            tool_output_max_lines: 200,
            oversize_abs_tokens: 6000,
            oversize_budget_ratio: 0.20,
        }
    }
}

impl CompactionConfig {
    /// Build from the existing `ContextConfig`.
    pub fn from_context_config(ctx: &crate::context::tracking::ContextConfig) -> Self {
        let budget_tokens = ctx
            .max_context_tokens
            .saturating_sub(ctx.system_prompt_tokens);
        let defaults = Self::default();
        Self {
            budget_tokens,
            system_prompt_tokens: ctx.system_prompt_tokens,
            compact_trigger_pct: ctx.compact_trigger_pct,
            compact_target_pct: ctx.compact_target_pct,
            keep_recent: ctx.keep_recent,
            keep_first: ctx.keep_first,
            max_messages: ctx.max_messages,
            message_limit_target_pct: ctx.message_limit_target_pct,
            microcompact_keep_tokens: ctx.microcompact_keep_tokens,
            microcompact_keep_images: defaults.microcompact_keep_images,
            tool_output_max_lines: ctx.tool_output_max_lines,
            oversize_abs_tokens: defaults.oversize_abs_tokens,
            oversize_budget_ratio: defaults.oversize_budget_ratio,
        }
    }

    /// Token threshold that triggers budget-based compaction.
    pub fn compact_trigger(&self) -> usize {
        self.budget_tokens * self.compact_trigger_pct as usize / 100
    }

    /// Token target after compaction.
    pub fn compact_target(&self) -> usize {
        self.budget_tokens * self.compact_target_pct as usize / 100
    }
}
