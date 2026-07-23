//! Compaction controller — integrates trigger, planner, and executor into the agent loop.

use tokio_util::sync::CancellationToken;

use super::config::CompactionConfig;
use super::executor;
use super::planner;
use super::summarizer::mode::SummarizerContext;
use super::trigger::TriggerInput;
use super::trigger::{self};
use super::types::*;
use crate::types::AgentMessage;

/// Stateful controller that lives across turns in the agent loop.
pub struct CompactionController {
    config: CompactionConfig,
    overflow_recovery_attempted: bool,
    last_compaction_ts: Option<u64>,
    /// Cross-compaction state (previous summary, cumulative file ops).
    /// Seeded from the session's last compaction on resume so follow-up
    /// compactions update the existing summary instead of restarting.
    state: CompactionState,
    observer: Option<CompactionObserver>,
}

impl CompactionController {
    pub fn new(config: CompactionConfig) -> Self {
        Self {
            config,
            overflow_recovery_attempted: false,
            last_compaction_ts: None,
            state: CompactionState::default(),
            observer: None,
        }
    }

    /// Seed cross-compaction state (e.g. restored from a persisted session).
    pub fn with_state(mut self, state: CompactionState) -> Self {
        self.state = state;
        self
    }

    /// Observe live phases for automatic compaction.
    pub fn with_observer(mut self, observer: CompactionObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Current cross-compaction state (for persistence by the caller).
    pub fn state(&self) -> &CompactionState {
        &self.state
    }

    /// Access current config.
    pub fn config(&self) -> &CompactionConfig {
        &self.config
    }

    /// Call after an accepted, non-error, non-aborted assistant response to
    /// reset the overflow flag.
    pub fn on_success(&mut self) {
        self.overflow_recovery_attempted = false;
    }

    /// Evaluate whether compaction should run after an assistant response.
    /// If compaction runs, mutates `messages` in place.
    /// Returns what the loop should do next.
    pub async fn after_response(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        usage: &UsageSnapshot,
        current_model: &ModelId,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        let trigger_input = TriggerInput {
            usage: Some(usage.clone()),
            current_model: current_model.clone(),
            last_compaction_ts: self.last_compaction_ts,
            overflow_recovery_attempted: self.overflow_recovery_attempted,
        };

        match trigger::evaluate(&trigger_input, &self.config) {
            TriggerDecision::Skip => CompactionResponse::skip(),

            TriggerDecision::Overflow { context_tokens } => {
                self.overflow_recovery_attempted = true;
                // Remove the error assistant message before compacting.
                if let Some(last) = messages.last() {
                    if matches!(
                        last,
                        AgentMessage::Llm(crate::types::Message::Assistant { .. })
                    ) {
                        messages.pop();
                    }
                }
                // Overflow still tries provider-native compaction first. If it
                // fails, use the deterministic emergency summary so recovery
                // stays fast and cannot fail on a second model call.
                let stats = self
                    .run_compaction(messages, summarizer_ctx, false, cancel)
                    .await;
                CompactionResponse {
                    action: AfterResponseAction::Retry,
                    stats,
                    reason: Some(CompactReason::Overflow),
                    context_tokens: Some(context_tokens),
                    overflow_exhausted: false,
                }
            }

            TriggerDecision::OverflowExhausted { context_tokens } => {
                // A compact-and-retry was already attempted this turn and the
                // context still overflows. Do not retry again — signal the loop
                // to surface a user-visible message.
                CompactionResponse {
                    action: AfterResponseAction::Continue,
                    stats: None,
                    reason: Some(CompactReason::Overflow),
                    context_tokens: Some(context_tokens),
                    overflow_exhausted: true,
                }
            }

            TriggerDecision::Threshold { context_tokens } => {
                let stats = self
                    .run_compaction(messages, summarizer_ctx, true, cancel)
                    .await;
                CompactionResponse {
                    action: AfterResponseAction::Continue,
                    stats,
                    reason: Some(CompactReason::Threshold),
                    context_tokens: Some(context_tokens),
                    overflow_exhausted: false,
                }
            }
        }
    }

    /// Estimate-based threshold compaction.
    ///
    /// Used when no reliable provider usage is available — either before the
    /// first prompt of a near-full resumed session, or after a non-overflow
    /// provider error whose response carries no usable token counts. Mirrors
    /// pi-mono's `_checkCompaction` error-estimate path: compact on the
    /// threshold using the caller-supplied estimate. Overflow detection is not
    /// attempted here because it requires real usage.
    pub async fn compact_on_estimate(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        estimated_tokens: usize,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        if estimated_tokens > self.config.trigger_threshold() {
            let stats = self
                .run_compaction(messages, summarizer_ctx, true, cancel)
                .await;
            return CompactionResponse {
                action: AfterResponseAction::Continue,
                stats,
                reason: Some(CompactReason::Threshold),
                context_tokens: Some(estimated_tokens),
                overflow_exhausted: false,
            };
        }

        CompactionResponse::skip()
    }

    /// Force a compaction (e.g., manual trigger from user command).
    pub async fn force_compact(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> Option<CompactionStats> {
        self.run_compaction(messages, summarizer_ctx, true, cancel)
            .await
    }

    async fn run_compaction(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        summarizer_ctx: Option<&SummarizerContext>,
        use_llm_fallback: bool,
        cancel: CancellationToken,
    ) -> Option<CompactionStats> {
        // A resumed context already contains the previous summary as a user
        // message. Remove only the exact message recorded in state so the
        // summarizer receives it once via `previous_summary`, not again as
        // ordinary conversation text.
        let removed_summary = self
            .state
            .context_summary_message
            .as_deref()
            .and_then(|summary| {
                messages
                    .iter()
                    .position(|message| is_exact_user_text(message, summary))
            })
            .map(|index| (index, messages.remove(index)));

        let Some(plan) = planner::plan(messages, &self.config) else {
            restore_removed_summary(messages, removed_summary);
            return None;
        };
        notify_compaction_phase(&self.observer, CompactionPhase::Planning);

        // For overflow, summarizer_ctx is None which triggers emergency summary.
        let outcome = executor::execute_with_options(
            std::mem::take(messages),
            &plan,
            &self.config,
            Some(&self.state),
            summarizer_ctx,
            executor::ExecutionOptions {
                use_llm_fallback,
                observer: self.observer.clone(),
                cancel: cancel.clone(),
            },
        )
        .await;

        // If stats are default (all zeros), LLM failed — compaction didn't happen.
        if outcome.stats.before_message_count == 0 {
            *messages = outcome.messages;
            restore_removed_summary(messages, removed_summary);
            if !cancel.is_cancelled() {
                notify_compaction_phase(&self.observer, CompactionPhase::Complete);
            }
            return None;
        }

        *messages = outcome.messages;
        self.state = outcome.state;
        self.last_compaction_ts = Some(now_ms());
        notify_compaction_phase(&self.observer, CompactionPhase::Complete);

        Some(outcome.stats)
    }
}

fn is_exact_user_text(message: &AgentMessage, expected: &str) -> bool {
    let AgentMessage::Llm(crate::types::Message::User { content, .. }) = message else {
        return false;
    };
    matches!(content.as_slice(), [crate::types::Content::Text { text }] if text == expected)
}

fn restore_removed_summary(
    messages: &mut Vec<AgentMessage>,
    removed: Option<(usize, AgentMessage)>,
) {
    if let Some((index, message)) = removed {
        messages.insert(index.min(messages.len()), message);
    }
}

/// Response from the compaction controller to the agent loop.
pub struct CompactionResponse {
    /// What the loop should do next.
    pub action: AfterResponseAction,
    /// Stats if compaction ran, None if skipped or nothing to evict.
    pub stats: Option<CompactionStats>,
    pub reason: Option<CompactReason>,
    pub context_tokens: Option<usize>,
    /// Set when overflow recovery was already attempted this turn and the
    /// context still overflows. The loop should surface this to the user.
    pub overflow_exhausted: bool,
}

impl CompactionResponse {
    fn skip() -> Self {
        Self {
            action: AfterResponseAction::Continue,
            stats: None,
            reason: None,
            context_tokens: None,
            overflow_exhausted: false,
        }
    }
}

fn now_ms() -> u64 {
    crate::context::now_ms()
}
