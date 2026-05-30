//! Trigger — decides whether compaction should run and why.

use super::config::CompactionConfig;
use super::types::ModelId;
use super::types::TriggerDecision;
use super::types::UsageSnapshot;
use crate::types::StopReason;

/// Input for trigger evaluation, collected from the agent loop.
#[derive(Debug, Clone)]
pub struct TriggerInput {
    /// Most recent assistant response usage.
    pub usage: Option<UsageSnapshot>,
    /// Current model identity.
    pub current_model: ModelId,
    /// Timestamp of the last compaction (to skip stale usage).
    pub last_compaction_ts: Option<u64>,
    /// Whether overflow recovery was already attempted this turn.
    pub overflow_recovery_attempted: bool,
}

/// Evaluate whether compaction should trigger.
pub fn evaluate(input: &TriggerInput, config: &CompactionConfig) -> TriggerDecision {
    let usage = match &input.usage {
        Some(u) => u,
        None => return TriggerDecision::Skip,
    };

    // Skip aborted responses — no valid usage data.
    if usage.stop_reason == StopReason::Aborted {
        return TriggerDecision::Skip;
    }

    // Model mismatch — usage from a different model (user switched models).
    if usage.model != input.current_model {
        return TriggerDecision::Skip;
    }

    // Stale usage — this response predates the last compaction.
    // Skip the check if timestamp is 0 (clock unavailable).
    if let Some(last_ts) = input.last_compaction_ts {
        if usage.timestamp > 0 && last_ts > 0 && usage.timestamp <= last_ts {
            return TriggerDecision::Skip;
        }
    }

    // Case 1: Overflow error — compact and retry.
    if usage.stop_reason == StopReason::Error {
        if let Some(ref err) = usage.error_message {
            if is_context_overflow(err) {
                let context_tokens = calculate_context_tokens(usage);
                if input.overflow_recovery_attempted {
                    return TriggerDecision::OverflowExhausted { context_tokens };
                }
                return TriggerDecision::Overflow { context_tokens };
            }
        }
        // Non-overflow errors: still check threshold via estimation.
        // Error responses may not have valid usage, so skip.
        return TriggerDecision::Skip;
    }

    // Case 2: Silent overflow — usage.input exceeds context window.
    let context_tokens = calculate_context_tokens(usage);
    if usage.stop_reason == StopReason::Stop && context_tokens > config.context_window {
        if input.overflow_recovery_attempted {
            return TriggerDecision::OverflowExhausted { context_tokens };
        }
        return TriggerDecision::Overflow { context_tokens };
    }

    // Case 3: Length-stop overflow (server truncated input, zero output).
    if usage.stop_reason == StopReason::Length && usage.output == 0 {
        let input_tokens = usage.input + usage.cache_read;
        if input_tokens >= config.context_window * 99 / 100 {
            if input.overflow_recovery_attempted {
                return TriggerDecision::OverflowExhausted { context_tokens };
            }
            return TriggerDecision::Overflow { context_tokens };
        }
    }

    // Case 4: Threshold — context approaching limit.
    if context_tokens > config.trigger_threshold() {
        return TriggerDecision::Threshold { context_tokens };
    }

    TriggerDecision::Skip
}

fn calculate_context_tokens(usage: &UsageSnapshot) -> usize {
    usage.input + usage.cache_read + usage.cache_write + usage.output
}

/// Match provider error messages that indicate context overflow.
fn is_context_overflow(error_message: &str) -> bool {
    static PATTERNS: &[&str] = &[
        // Anthropic
        "prompt is too long",
        "request_too_large",
        "request exceeds the maximum size",
        // OpenAI
        "exceeds the context window",
        "exceeds the model's maximum context length",
        // Google
        "exceeds the maximum number of tokens allowed",
        "input token count",
        // xAI
        "maximum prompt length is",
        // Groq
        "reduce the length of the messages",
        // OpenRouter
        "maximum context length is",
        // Together AI
        "is longer than the model's context length",
        // llama.cpp
        "exceeds the available context size",
        // LM Studio
        "greater than the context length",
        // Poolside
        "exceeds the maximum allowed input length",
        // GitHub Copilot
        "prompt token count of",
        // MiniMax
        "context window exceeds limit",
        // Kimi
        "exceeded model token limit",
        // Mistral
        "too large for model with",
        // Cerebras (status code only, but sometimes has body)
        "request too large",
    ];

    let lower = error_message.to_lowercase();
    PATTERNS.iter().any(|p| lower.contains(p))
}
