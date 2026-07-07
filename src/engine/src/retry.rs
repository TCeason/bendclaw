//! Retry policy for transient provider errors.
//!
//! Defines [`RetryPolicy`] (backoff timing) and [`should_retry()`] (error
//! classification). The agent loop combines both to decide whether and
//! when to re-attempt a failed provider call.

use std::time::Duration;

use crate::provider::ProviderError;

/// Retry policy with exponential backoff.
///
/// Controls *how many* times and *how long* to wait between retries.
/// Use [`RetryPolicy::disabled()`] to fail immediately on any error.
///
/// Internal backoff parameters (2 s initial, 2× multiplier, 30 s cap,
/// ±20 % jitter) are intentionally not exposed — callers express intent
/// via [`new()`](RetryPolicy::new) and the
/// implementation is free to evolve.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    max_retries: usize,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_retries: 10 }
    }
}

// Internal backoff constants.
const INITIAL_DELAY_MS: f64 = 2000.0;
const BACKOFF_MULTIPLIER: f64 = 2.0;
const MAX_DELAY_MS: f64 = 30_000.0;

impl RetryPolicy {
    /// No retries — fail immediately on any error.
    pub fn disabled() -> Self {
        Self { max_retries: 0 }
    }

    /// Create a policy that retries up to `n` times.
    pub fn new(n: usize) -> Self {
        Self { max_retries: n }
    }

    /// Maximum number of retry attempts (0 = no retries).
    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    /// Calculate the delay for a given attempt (1-indexed).
    /// Uses exponential backoff with ±20 % jitter.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let base_ms = INITIAL_DELAY_MS * BACKOFF_MULTIPLIER.powi((attempt - 1) as i32);
        let capped_ms = base_ms.min(MAX_DELAY_MS);

        // Jitter: ±20 % (multiply by 0.8–1.2)
        let jitter = 0.8 + rand::random::<f64>() * 0.4;
        Duration::from_millis((capped_ms * jitter) as u64)
    }
}

/// Whether this provider error is safe to retry.
///
/// Retryable: rate limits (429), network/transient errors, overloaded (529),
/// and explicit transient API messages.
/// Not retryable: auth (401/403), context overflow, cancellation,
/// client errors (400 etc.), not found (404).
pub fn should_retry(error: &ProviderError) -> bool {
    match error {
        ProviderError::RateLimited { .. }
        | ProviderError::Network(_)
        | ProviderError::Overloaded(_) => true,
        // A bare Api error that is really a context overflow must never retry,
        // even if its wording also contains a transient phrase like "try again".
        // Overflow is handled by compaction, not retry.
        ProviderError::Api(message) => {
            !crate::provider::error::is_context_overflow_message(message)
                && is_retryable_api_message(message)
        }
        _ => false,
    }
}

fn is_retryable_api_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("overloaded")
        || lower.contains("try again")
        || lower.contains("temporarily unavailable")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("internal server error")
        || lower.contains("server error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
        || lower.contains("stream interrupted")
        || lower.contains("please retry")
        // An empty 200 response (no content, no usage) is a transient provider
        // or proxy defect — the request was accepted but nothing came back.
        // Both SSE decoders surface it as an Api error; retrying mirrors pi,
        // which classifies interrupted/empty streams as retryable.
        || lower.contains("empty response from provider")
        // Malformed tool-call output is a transient model defect, not a client
        // error: sampling is non-deterministic, so re-running the same call
        // usually yields valid JSON. Common with smaller local models. Retrying
        // is far better than surfacing a fatal error to the user.
        || lower.contains("malformed tool_use")
        || lower.contains("invalid_tool_call")
        || lower.contains("could not recover a valid tool call")
        || lower.contains("http 500")
        || lower.contains("http 502")
        || lower.contains("http 503")
        || lower.contains("http 504")
}
