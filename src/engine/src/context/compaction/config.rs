//! Compaction configuration — single source of truth for all tunable parameters.

use super::summarizer::SummarizerMode;

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
    /// The sole retention condition, matching pi's `keepRecentTokens`.
    pub keep_recent_tokens: usize,

    // — Summarizer —
    /// Summarization strategy for summary generation.
    pub summarizer_mode: SummarizerMode,
    /// Maximum UTF-8 bytes retained in a generated summary.
    pub summary_max_bytes: usize,
}

/// Tokens reserved for output + system prompt + tool defs. Fixed rather than a
/// share of the window, mirroring pi's `DEFAULT_COMPACTION_SETTINGS`: the
/// reserve covers a response, not a fraction of history, so scaling it with the
/// window over-reserves on large-context models.
pub const DEFAULT_RESERVE_TOKENS: usize = 16_384;
/// Token budget for the retained tail. Also fixed, for the same reason.
pub const DEFAULT_KEEP_RECENT_TOKENS: usize = 20_000;
/// Maximum UTF-8 bytes for a summary inserted back into model context. Provider
/// output limits are token-based and do not protect deterministic fallbacks or
/// oversized summaries restored from older sessions.
pub const DEFAULT_SUMMARY_MAX_BYTES: usize = 64 * 1024;
/// Absolute byte ceiling for each complete summarizer user prompt. The effective
/// budget is also bounded by the model's context window via
/// `summarizer_input_max_bytes()`.
pub const SUMMARIZER_INPUT_MAX_BYTES: usize = 512 * 1024;
const ESTIMATED_BYTES_PER_TOKEN: usize = 4;

impl CompactionConfig {
    /// Derive config from a context config (uses max_context_tokens as the window).
    pub fn from_context_window(context_window: usize) -> Self {
        Self {
            context_window,
            reserve_tokens: DEFAULT_RESERVE_TOKENS,
            keep_recent_tokens: DEFAULT_KEEP_RECENT_TOKENS,
            summarizer_mode: SummarizerMode::default(),
            summary_max_bytes: DEFAULT_SUMMARY_MAX_BYTES,
        }
    }

    /// Build from a `ContextConfig`, respecting user-customized fields.
    pub fn from_context_config(ctx: &crate::context::tracking::ContextConfig) -> Self {
        // ContextConfig.max_context_tokens is the full context window.
        // Output headroom is reserved here via reserve_tokens (single source
        // of headroom), so trigger threshold = window - reserve_tokens.
        let mut cfg = Self::from_context_window(ctx.max_context_tokens);
        if let Some(reserve) = ctx.reserve_tokens {
            cfg.reserve_tokens = reserve;
        }
        if let Some(keep_recent) = ctx.keep_recent_tokens {
            cfg.keep_recent_tokens = keep_recent;
        }
        cfg
    }

    /// Input budget for each complete summarizer user prompt. This follows pi's
    /// branch-summary budget (`context window - reserve`) and adds a transport
    /// ceiling so compaction cannot emit another multi-megabyte request.
    pub fn summarizer_input_max_bytes(&self) -> usize {
        if self.context_window == 0 {
            return SUMMARIZER_INPUT_MAX_BYTES;
        }
        self.context_window
            .saturating_sub(self.reserve_tokens)
            .saturating_mul(ESTIMATED_BYTES_PER_TOKEN)
            .min(SUMMARIZER_INPUT_MAX_BYTES)
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
