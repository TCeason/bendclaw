//! Compaction controller — integrates trigger, planner, and executor into the agent loop.

use tokio_util::sync::CancellationToken;

use super::config::CompactionConfig;
use super::executor;
use super::executor::ExecutionResult;
use super::plan;
use super::summarizer::mode::SummarizerContext;
use super::summary::LlmPolicy;
use super::summary::SummaryContexts;
use super::trigger::TriggerInput;
use super::trigger::{self};
use super::types::*;
use crate::types::AgentMessage;

/// Threshold-compaction suppression key (droid-style): once a threshold
/// compaction failed to bring usage back under the limit, further threshold
/// compactions for the same model/threshold are skipped until usage recovers.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ThresholdKey {
    model: ModelId,
    threshold: usize,
}

/// Stateful controller that lives across turns in the agent loop.
pub struct CompactionController {
    config: CompactionConfig,
    overflow_recovery_attempted: bool,
    last_compaction_ts: Option<u64>,
    threshold_suppression: Option<ThresholdKey>,
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
            threshold_suppression: None,
            state: CompactionState::default(),
            observer: None,
        }
    }

    /// Seed cross-compaction state (e.g. restored from a persisted session).
    pub fn with_state(mut self, state: CompactionState) -> Self {
        self.last_compaction_ts = (state.timestamp > 0).then_some(state.timestamp);
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

    /// Start a new user turn. Overflow recovery is limited per user turn, so a
    /// newly accepted user message resets the compact-and-retry allowance.
    pub fn on_user_message(&mut self) {
        self.overflow_recovery_attempted = false;
    }

    /// Call when an assistant response other than an error completes. This
    /// mirrors pi's `message_end` lifecycle: stop, length, tool-use, and aborted
    /// responses all clear the previous compact-and-retry attempt immediately.
    pub fn on_non_error_response(&mut self) {
        self.overflow_recovery_attempted = false;
    }

    /// Evaluate compaction using one model context for both remote and local
    /// summary stages.
    pub async fn after_response(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        usage: &UsageSnapshot,
        current_model: &ModelId,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        self.after_response_with_contexts(
            messages,
            usage,
            current_model,
            SummaryContexts::same(summarizer_ctx),
            cancel,
        )
        .await
    }

    /// Evaluate whether compaction should run after an assistant response.
    /// Remote and local LLM stages may use different model contexts.
    pub async fn after_response_with_contexts(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        usage: &UsageSnapshot,
        current_model: &ModelId,
        contexts: SummaryContexts<'_>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        let trigger_input = TriggerInput {
            usage: Some(usage.clone()),
            current_model: current_model.clone(),
            last_compaction_ts: self.last_compaction_ts,
            overflow_recovery_attempted: self.overflow_recovery_attempted,
        };
        self.clear_suppression_if_recovered(usage, current_model);

        match trigger::evaluate(&trigger_input, &self.config) {
            TriggerDecision::Skip => CompactionResponse::skip(),

            TriggerDecision::Overflow {
                context_tokens,
                will_retry,
            } => {
                if will_retry
                    && matches!(
                        messages.last(),
                        Some(AgentMessage::Llm(crate::types::Message::Assistant { .. }))
                    )
                {
                    messages.pop();
                }

                // Match pi: overflow first uses the normal compaction pipeline.
                // The provider request is retried only after compaction
                // completed; cancellation or no plan leaves the turn terminal
                // instead of resending the same oversized context.
                let request_overhead_tokens = contexts.request_overhead_tokens();
                let mut stats = self
                    .run_compaction(
                        messages,
                        contexts,
                        request_overhead_tokens,
                        LlmPolicy::Required,
                        cancel.clone(),
                    )
                    .await;
                if will_retry && stats.is_none() && !cancel.is_cancelled() {
                    // Recovery must not depend on a second model call: the
                    // summarize request goes to the same provider that just
                    // rejected the oversized payload and can fail for the
                    // same reason (e.g. a relay byte limit). Divergence from
                    // pi, which gives up here: evot falls back to the
                    // deterministic emergency summary so one compact-and-retry
                    // still happens.
                    stats = self
                        .run_compaction(
                            messages,
                            SummaryContexts::default(),
                            request_overhead_tokens,
                            LlmPolicy::Skip,
                            cancel.clone(),
                        )
                        .await;
                }
                let retry_after_compaction = will_retry && stats.is_some();
                if retry_after_compaction {
                    self.overflow_recovery_attempted = true;
                }

                CompactionResponse {
                    action: if retry_after_compaction {
                        AfterResponseAction::Retry
                    } else {
                        AfterResponseAction::Continue
                    },
                    stats,
                    reason: Some(CompactReason::Overflow),
                    context_tokens: Some(context_tokens),
                    overflow_exhausted: false,
                    overflow_recovery_failed: will_retry
                        && !retry_after_compaction
                        && !cancel.is_cancelled(),
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
                    overflow_recovery_failed: false,
                }
            }

            TriggerDecision::Threshold { context_tokens } => {
                self.threshold_compact(messages, context_tokens, current_model, contexts, cancel)
                    .await
            }
        }
    }

    /// Estimate-driven threshold check using one model for all summary stages.
    pub async fn compact_on_estimate(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        estimated_tokens: usize,
        current_model: &ModelId,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        self.compact_on_estimate_with_contexts(
            messages,
            estimated_tokens,
            current_model,
            SummaryContexts::same(summarizer_ctx),
            cancel,
        )
        .await
    }

    /// Estimate-driven threshold check with independent summary contexts.
    pub async fn compact_on_estimate_with_contexts(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        estimated_tokens: usize,
        current_model: &ModelId,
        contexts: SummaryContexts<'_>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        if self.config.context_window == 0 {
            return CompactionResponse::skip();
        }
        if estimated_tokens < self.config.trigger_threshold() {
            self.clear_suppression_for(current_model);
            return CompactionResponse::skip();
        }
        self.threshold_compact(messages, estimated_tokens, current_model, contexts, cancel)
            .await
    }

    /// Force a compaction (e.g., manual trigger from user command).
    pub async fn force_compact(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> Option<CompactionStats> {
        let contexts = SummaryContexts::same(summarizer_ctx);
        let request_overhead_tokens = contexts.request_overhead_tokens();
        self.run_compaction(
            messages,
            contexts,
            request_overhead_tokens,
            LlmPolicy::Required,
            cancel,
        )
        .await
    }

    /// Shared threshold path: honor suppression, compact, then arm suppression
    /// so a compaction that could not lower usage does not repeat every turn.
    async fn threshold_compact(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        context_tokens: usize,
        current_model: &ModelId,
        contexts: SummaryContexts<'_>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        let key = ThresholdKey {
            model: current_model.clone(),
            threshold: self.config.trigger_threshold(),
        };
        if self.threshold_suppression.as_ref() == Some(&key) {
            tracing::debug!(
                context_tokens,
                threshold = key.threshold,
                "threshold compaction suppressed: previous compaction did not lower usage"
            );
            return CompactionResponse::skip();
        }

        let request_overhead_tokens = contexts.request_overhead_tokens();
        let stats = self
            .run_compaction(
                messages,
                contexts,
                request_overhead_tokens,
                LlmPolicy::Required,
                cancel,
            )
            .await;
        if stats.is_some() {
            self.threshold_suppression = Some(key);
        }
        CompactionResponse {
            action: AfterResponseAction::Continue,
            stats,
            reason: Some(CompactReason::Threshold),
            context_tokens: Some(context_tokens),
            overflow_exhausted: false,
            overflow_recovery_failed: false,
        }
    }

    /// Post-compaction usage back under the threshold re-arms threshold
    /// compaction. Stale usage (predating the last compaction) is ignored.
    fn clear_suppression_if_recovered(&mut self, usage: &UsageSnapshot, current_model: &ModelId) {
        if self.config.context_window == 0 || usage.model != *current_model {
            return;
        }
        if let Some(last_ts) = self.last_compaction_ts {
            if usage.timestamp > 0 && last_ts > 0 && usage.timestamp <= last_ts {
                return;
            }
        }
        if trigger::context_tokens(usage) <= self.config.trigger_threshold() {
            self.clear_suppression_for(current_model);
        }
    }

    fn clear_suppression_for(&mut self, current_model: &ModelId) {
        if self
            .threshold_suppression
            .as_ref()
            .is_some_and(|key| key.model == *current_model)
        {
            self.threshold_suppression = None;
        }
    }

    async fn run_compaction(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        contexts: SummaryContexts<'_>,
        request_overhead_tokens: usize,
        llm_policy: LlmPolicy,
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

        let retained_tail = self.config.retained_tail_budget(request_overhead_tokens);
        let Some(plan) = plan::plan_messages(messages, retained_tail) else {
            restore_removed_summary(messages, removed_summary);
            return None;
        };
        notify_compaction_phase(&self.observer, CompactionPhase::Planning);

        let result = executor::execute_with_contexts(
            std::mem::take(messages),
            &plan,
            &self.config,
            Some(&self.state),
            contexts,
            executor::ExecutionOptions {
                llm_policy,
                observer: self.observer.clone(),
                cancel: cancel.clone(),
            },
        )
        .await;

        match result {
            ExecutionResult::Skipped(returned) => {
                *messages = returned;
                restore_removed_summary(messages, removed_summary);
                if !cancel.is_cancelled() {
                    notify_compaction_phase(&self.observer, CompactionPhase::Complete);
                }
                None
            }
            ExecutionResult::Compacted(outcome) => {
                *messages = outcome.messages;
                self.state = outcome.state;
                self.last_compaction_ts = Some(self.state.timestamp);
                notify_compaction_phase(&self.observer, CompactionPhase::Complete);
                Some(outcome.stats)
            }
        }
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
    /// Set when an overflow demanded a compact-and-retry but compaction could
    /// not run (nothing to evict). The loop should surface this to the user
    /// instead of failing silently.
    pub overflow_recovery_failed: bool,
}

impl CompactionResponse {
    fn skip() -> Self {
        Self {
            action: AfterResponseAction::Continue,
            stats: None,
            reason: None,
            context_tokens: None,
            overflow_exhausted: false,
            overflow_recovery_failed: false,
        }
    }
}
