//! Compaction configuration — single source of truth for all tunable parameters.

use super::summarizer::SummarizerMode;

/// All compaction parameters in one place.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    // — Trigger —
    /// Context window size (total tokens the model accepts).
    pub context_window: usize,
    /// Tokens reserved for output + system prompt + tool defs.
    /// Trigger threshold = context_window - reserve_tokens.
    pub reserve_tokens: usize,

    // — Planner —
    /// Token budget for the retained tail (recent messages to keep in full).
    pub keep_recent_tokens: usize,
    /// Minimum messages to keep in the tail (floor guarantee).
    pub keep_recent_min: usize,
    /// Fixed head messages to always keep (first user + assistant pair).
    pub keep_first: usize,

    // — Shrink —
    /// Absolute token threshold for a single tool result to be considered oversized.
    pub max_tool_result_tokens: usize,
    /// Max lines per tool output after truncation.
    pub tool_output_max_lines: usize,

    // — Reclaim —
    /// Keep this many most-recent images at full content; older ones get downgraded.
    pub keep_recent_images: usize,

    // — Summarizer —
    /// Summarization strategy for marker generation.
    pub summarizer_mode: SummarizerMode,
    /// Max chars for stored last_summary (prevents unbounded growth).
    pub summary_max_chars: usize,
}

impl CompactionConfig {
    /// Derive config from a context config (uses max_context_tokens as the window).
    pub fn from_context_window(context_window: usize) -> Self {
        Self {
            context_window,
            reserve_tokens: context_window / 8, // ~12.5% reserve
            keep_recent_tokens: context_window / 5,
            keep_recent_min: 6,
            keep_first: 2,
            max_tool_result_tokens: 6_000,
            tool_output_max_lines: 200,
            keep_recent_images: 2,
            summarizer_mode: SummarizerMode::default(),
            summary_max_chars: 4000,
        }
    }

    /// Build from a `ContextConfig`, respecting user-customized fields.
    pub fn from_context_config(ctx: &crate::context::tracking::ContextConfig) -> Self {
        // ContextConfig.max_context_tokens is already the usable budget
        // (typically 80% of real window). Treat it as our context_window.
        let context_window = ctx.max_context_tokens;
        let mut cfg = Self::from_context_window(context_window);
        cfg.keep_first = ctx.keep_first;
        cfg.tool_output_max_lines = ctx.tool_output_max_lines;
        cfg.keep_recent_min = ctx.keep_recent;
        cfg
    }

    /// Token threshold that triggers compaction.
    pub fn trigger_threshold(&self) -> usize {
        self.context_window.saturating_sub(self.reserve_tokens)
    }
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self::from_context_window(128_000)
    }
}
