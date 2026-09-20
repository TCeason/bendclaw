//! How much one judge request may carry, and how to size text against it.
//!
//! A judge is a model with a context window; every caller that builds a
//! state must fit it. The numbers live with the judge, not the caller, so a
//! new judge model is adopted by reporting its own limits and nothing in the
//! callers changes.

/// The size one request to a judge may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JudgeLimits {
    /// Tokens one request (state plus questions) may use, as counted by
    /// [`estimate_tokens`] or the judge's own estimator. Already leaves
    /// headroom under the real window for the estimate being an estimate.
    pub request_tokens: usize,
    /// Questions one request may carry.
    pub max_questions: usize,
}

impl JudgeLimits {
    /// TypeSafe Jev: 32k window.
    pub const JEV: Self = Self::for_window(32_000);

    /// Limits for a judge with the given context window: two thirds of it is
    /// a safe request, the rest covers the estimate's error and the reply.
    pub const fn for_window(window_tokens: usize) -> Self {
        Self {
            request_tokens: window_tokens / 3 * 2,
            max_questions: 40,
        }
    }
}

impl Default for JudgeLimits {
    fn default() -> Self {
        Self::JEV
    }
}

/// Tokens, estimated by script: a CJK character is about one token, other
/// text about four characters a token. Deliberately pessimistic, and good
/// enough for any judge whose tokenizer is in the usual family; a judge with
/// an unusual one overrides [`Judge::estimate_tokens`].
///
/// [`Judge::estimate_tokens`]: super::Judge::estimate_tokens
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        let c = ch as u32;
        let is_cjk = (0x3000..=0x9FFF).contains(&c)
            || (0xAC00..=0xD7AF).contains(&c)
            || (0xF900..=0xFAFF).contains(&c)
            || (0xFF00..=0xFFEF).contains(&c)
            || (0x20000..=0x3134F).contains(&c);
        if is_cjk {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}
