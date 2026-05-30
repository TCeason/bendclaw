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
            TriggerDecision::Skip => CompactionResponse {
                action: AfterResponseAction::Continue,
                stats: None,
                reason: None,
                context_tokens: None,
            },

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
                }
            }
        }
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
}

fn now_ms() -> u64 {
    crate::context::now_ms()
}
