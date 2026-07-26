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

    // Stale usage — this response predates the last compaction.
    // Skip the check if timestamp is 0 (clock unavailable).
    if let Some(last_ts) = input.last_compaction_ts {
        if usage.timestamp > 0 && last_ts > 0 && usage.timestamp <= last_ts {
            return TriggerDecision::Skip;
        }
    }

    let context_tokens = calculate_context_tokens(usage);
    // A model change is not a compaction signal. The old model's usage and
    // errors describe a different context boundary; wait for the selected
    // model's own usage or an explicit overflow from that model.
    if usage.model != input.current_model {
        return TriggerDecision::Skip;
    }
    // pi gates every window-relative signal behind `contextWindow &&`. With an
    // unknown window (custom/proxy models), only explicit overflow errors may
    // trigger; otherwise any successful usage would read as a silent overflow.
    let window_known = config.context_window > 0;

    // Case 1: Explicit overflow errors from the selected model compact and retry.
    if usage.stop_reason == StopReason::Error {
        if usage
            .error_message
            .as_deref()
            .is_some_and(is_context_overflow)
        {
            if input.overflow_recovery_attempted {
                return TriggerDecision::OverflowExhausted { context_tokens };
            }
            return TriggerDecision::Overflow {
                context_tokens,
                will_retry: true,
            };
        }
        // Non-overflow errors use the caller's anchored estimate fallback.
        return TriggerDecision::Skip;
    }

    // Case 2: Successful silent overflow. pi detects this from input tokens,
    // not total usage. The completed answer is retained and compaction does not
    // retry from an assistant message.
    if window_known
        && usage.stop_reason == StopReason::Stop
        && usage.input + usage.cache_read > config.context_window
    {
        return TriggerDecision::Overflow {
            context_tokens,
            will_retry: false,
        };
    }

    // Case 3: Length-stop silent overflow. Providers such as MiMo may truncate
    // input to the window and return no output. Partial output is not an
    // overflow signal; it falls through to ordinary threshold compaction.
    if window_known && usage.stop_reason == StopReason::Length && usage.output == 0 {
        let input_tokens = usage.input + usage.cache_read;
        if input_tokens >= config.context_window * 99 / 100 {
            if input.overflow_recovery_attempted {
                return TriggerDecision::OverflowExhausted { context_tokens };
            }
            return TriggerDecision::Overflow {
                context_tokens,
                will_retry: true,
            };
        }
    }

    // Case 4: Threshold — context approaching limit.
    if window_known && context_tokens > config.trigger_threshold() {
        return TriggerDecision::Threshold { context_tokens };
    }

    TriggerDecision::Skip
}

fn calculate_context_tokens(usage: &UsageSnapshot) -> usize {
    if usage.total_tokens > 0 {
        usage.total_tokens
    } else {
        usage.input + usage.cache_read + usage.cache_write + usage.output
    }
}

/// Match provider error messages that indicate context overflow.
///
/// Delegates to the single source of truth in the provider error module so the
/// trigger, the HTTP classifier, and the retry policy never drift apart.
fn is_context_overflow(error_message: &str) -> bool {
    crate::provider::error::is_context_overflow_message(error_message)
}
