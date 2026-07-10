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
}

impl CompactionController {
    pub fn new(config: CompactionConfig) -> Self {
        Self {
            config,
            overflow_recovery_attempted: false,
            last_compaction_ts: None,
        }
    }

    /// Update config (e.g., when model changes and context_window differs).
    pub fn set_config(&mut self, config: CompactionConfig) {
        self.config = config;
    }

    /// Access current config.
    pub fn config(&self) -> &CompactionConfig {
        &self.config
    }

    /// Call after a successful (non-error, non-aborted) assistant response
    /// to reset the overflow flag.
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
                // Overflow always uses rule-based (fast, never fails)
                let stats = self.run_compaction(messages, None, cancel).await;
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
                self.overflow_recovery_attempted = false;
                let stats = self.run_compaction(messages, summarizer_ctx, cancel).await;
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

    /// Evaluate whether the context should be compacted *before* sending the
    /// next prompt to the provider.
    ///
    /// Primary path: mirror the post-response threshold check using the latest
    /// assistant provider usage retained in the transcript.
    ///
    /// Fallback path: when no usable provider usage is available (e.g. the last
    /// turns were all errors with zero usage), fall back to the caller-supplied
    /// `estimated_tokens` and compact on the threshold. Without this, a session
    /// that hits repeated errors can never recover because every error turn
    /// carries no usage and the pre-prompt check would skip forever. Mirrors
    /// pi-mono's `_checkCompaction` error-estimate path.
    pub async fn before_prompt(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        current_model: &ModelId,
        estimated_tokens: usize,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> CompactionResponse {
        if let Some(usage) = latest_assistant_usage(messages) {
            // Prefer real provider usage when it carries an input signal.
            if usage.input > 0 || usage.cache_read > 0 {
                return self
                    .after_response(messages, &usage, current_model, summarizer_ctx, cancel)
                    .await;
            }
        }

        // No usable usage — fall back to the local estimate for a threshold-only
        // decision. Overflow detection requires real usage, so it is not
        // attempted here.
        self.compact_on_estimate(messages, estimated_tokens, summarizer_ctx, cancel)
            .await
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
            self.overflow_recovery_attempted = false;
            let stats = self.run_compaction(messages, summarizer_ctx, cancel).await;
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
        self.run_compaction(messages, summarizer_ctx, cancel).await
    }

    async fn run_compaction(
        &mut self,
        messages: &mut Vec<AgentMessage>,
        summarizer_ctx: Option<&SummarizerContext>,
        cancel: CancellationToken,
    ) -> Option<CompactionStats> {
        let plan = planner::plan(messages, &self.config)?;

        // For overflow, summarizer_ctx is None which triggers emergency summary.
        let outcome = executor::execute(
            std::mem::take(messages),
            &plan,
            &self.config,
            None,
            summarizer_ctx,
            cancel,
        )
        .await;

        // If stats are default (all zeros), LLM failed — compaction didn't happen
        if outcome.stats.before_message_count == 0 {
            *messages = outcome.messages;
            return None;
        }

        *messages = outcome.messages;
        self.last_compaction_ts = Some(now_ms());

        Some(outcome.stats)
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

fn latest_assistant_usage(messages: &[AgentMessage]) -> Option<UsageSnapshot> {
    for message in messages.iter().rev() {
        let AgentMessage::Llm(crate::types::Message::Assistant {
            usage,
            stop_reason,
            model,
            provider,
            timestamp,
            error_message,
            ..
        }) = message
        else {
            continue;
        };
        return Some(UsageSnapshot {
            input: usage.input as usize,
            cache_read: usage.cache_read as usize,
            cache_write: usage.cache_write as usize,
            output: usage.output as usize,
            total_tokens: usage.total_tokens as usize,
            model: ModelId {
                provider: provider.clone(),
                model: model.clone(),
            },
            timestamp: *timestamp,
            stop_reason: stop_reason.clone(),
            error_message: error_message.clone(),
        });
    }
    None
}

fn now_ms() -> u64 {
    crate::context::now_ms()
}
