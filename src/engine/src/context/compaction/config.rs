//! Compaction configuration — single source of truth for all tunable parameters.

use super::summarizer::SummarizerMode;

/// Byte cap for a stored compaction summary. Sized for the structured
/// checkpoint format (Goal/Progress/Decisions/Next Steps/Critical Context):
/// 4 KB routinely amputated the trailing sections, which are exactly what the
/// next compaction's incremental update needs. 16 KB ≈ 4k tokens is still a
/// small fraction of any supported context window. (pi stores summaries
/// untruncated with a ~13k-token generation budget.)
pub const DEFAULT_SUMMARY_MAX_BYTES: usize = 16_000;
const SUMMARY_TRUNCATION_MARKER: &str = "\n\n[… compaction summary truncated …]\n\n";

/// Bound a compaction summary while retaining both its overview and latest
/// conclusion. The limit is a byte budget and the result is always valid UTF-8.
pub fn truncate_summary(summary: &str, max_bytes: usize) -> String {
    if summary.len() <= max_bytes {
        return summary.to_string();
    }
    if max_bytes == 0 {
        return String::new();
    }
    if max_bytes <= SUMMARY_TRUNCATION_MARKER.len() {
        return summary[..summary.floor_char_boundary(max_bytes)].to_string();
    }

    let content_budget = max_bytes - SUMMARY_TRUNCATION_MARKER.len();
    let head_budget = content_budget * 2 / 3;
    let head_end = summary.floor_char_boundary(head_budget);
    let tail_budget = content_budget - head_end;
    let mut tail_start = summary.len().saturating_sub(tail_budget);
    while tail_start < summary.len() && !summary.is_char_boundary(tail_start) {
        tail_start += 1;
    }

    format!(
        "{}{}{}",
        &summary[..head_end],
        SUMMARY_TRUNCATION_MARKER,
        &summary[tail_start..]
    )
}

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
    /// Summarization strategy for summary generation.
    pub summarizer_mode: SummarizerMode,
    /// Maximum UTF-8 bytes retained in a generated summary. The historical
    /// field name is kept to avoid churn in callers.
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
            summary_max_chars: DEFAULT_SUMMARY_MAX_BYTES,
        }
    }

    /// Build from a `ContextConfig`, respecting user-customized fields.
    pub fn from_context_config(ctx: &crate::context::tracking::ContextConfig) -> Self {
        // ContextConfig.max_context_tokens is the full context window.
        // Output headroom is reserved here via reserve_tokens (single source
        // of headroom), so trigger threshold = window - reserve_tokens.
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
