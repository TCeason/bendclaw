//! Prune policy. Anything about the judge model itself — its window, how it
//! counts tokens — comes from [`Judge::limits`] and is not repeated here.
//!
//! [`Judge::limits`]: crate::judge::Judge::limits

#[derive(Debug, Clone)]
pub struct PruneOptions {
    /// Newest messages never touched (the first message is always kept).
    pub preserve_recent: usize,
    /// Minimum keep probability for a call or a result to stay.
    pub keep_threshold: f64,
    /// Judge requests in flight at once during one decide round.
    pub decide_concurrency: usize,
    /// Characters of a dropped result kept before its note.
    pub truncate_head_chars: usize,
    /// Ask the judge again once the context grew by this many tokens.
    pub decide_growth_tokens: usize,
    /// Do not ask for fewer undecided candidates than this.
    pub decide_min_candidates: usize,
    /// Judge requests per decide round. A resumed multi-megatoken session has
    /// thousands of candidates; the oldest are asked first and the rest wait
    /// for the next round rather than stalling the run.
    pub max_requests_per_decide: usize,
    /// A `Keep` is asked again once the context grew by this many tokens.
    pub revisit_kept_tokens: usize,
}

impl Default for PruneOptions {
    fn default() -> Self {
        Self {
            preserve_recent: 6,
            keep_threshold: 0.5,
            decide_concurrency: 4,
            truncate_head_chars: 300,
            decide_growth_tokens: 15_000,
            decide_min_candidates: 8,
            max_requests_per_decide: 10,
            revisit_kept_tokens: 40_000,
        }
    }
}
