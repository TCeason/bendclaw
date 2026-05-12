//! Compaction orchestration — strategy-driven level pipeline.
//!
//! The orchestrator owns only pipeline concerns:
//!   1. build context snapshots,
//!   2. classify pressure,
//!   3. invoke level strategies in order,
//!   4. maintain accounting,
//!   5. sanitize and build stats.
//!
//! Level-specific decisions live in `levels.rs` and phase transforms.

use std::mem;

use super::accounting::build_stats;
use super::accounting::collect_tool_details;
use super::accounting::StatsInput;
use super::levels::default_levels;
use super::levels::LevelInput;
use super::phase::BudgetTargets;
use super::phase::PhaseContext;
use super::phase::RetentionBounds;
use super::phase::ShrinkSettings;
use super::policy::CompactionPolicy;
use super::pressure::PressureState;
use super::sanitize::sanitize_tool_pairs;
use super::snapshot::ContextSnapshot;
use super::types::CompactionResult;
use crate::context::tokens::total_tokens;
use crate::context::tracking::CompactionBudgetState;
use crate::context::tracking::ContextConfig;
use crate::types::*;

struct CompactionRuntime {
    messages: Vec<AgentMessage>,
    current_tokens: usize,
    estimated_tokens: usize,
    original_message_tokens: usize,
    original_estimated_tokens: usize,
    original_image_count: usize,
}

impl CompactionRuntime {
    fn budget_state(&self) -> CompactionBudgetState {
        CompactionBudgetState {
            estimated_tokens: self.estimated_tokens,
        }
    }

    fn observe_result(
        &mut self,
        result: super::phase::PhaseResult,
        snapshot: &ContextSnapshot,
        config: &ContextConfig,
    ) -> super::phase::PhaseResult {
        let saved = result
            .actions
            .iter()
            .map(|action| action.before_tokens.saturating_sub(action.after_tokens))
            .sum::<usize>();
        self.current_tokens = self.current_tokens.saturating_sub(saved);

        if self.original_image_count > 0 {
            let after_snapshot =
                ContextSnapshot::new(&result.messages, config, &CompactionBudgetState {
                    estimated_tokens: self.estimated_tokens,
                });
            let image_removed = snapshot
                .image_count
                .saturating_sub(after_snapshot.image_count);
            if image_removed > 0 {
                let provider_extra = self
                    .original_estimated_tokens
                    .saturating_sub(snapshot.message_tokens);
                let image_provider_extra =
                    provider_extra / self.original_image_count.max(1) * image_removed;
                self.current_tokens = self.current_tokens.saturating_sub(image_provider_extra);
            }
        }

        self.current_tokens = self.current_tokens.max(total_tokens(&result.messages));
        self.estimated_tokens = self
            .original_estimated_tokens
            .saturating_sub(
                self.original_message_tokens
                    .saturating_sub(self.current_tokens),
            )
            .max(self.current_tokens);
        self.messages = result.messages.clone();

        result
    }
}

/// Compact messages using a pressure-driven level pipeline.
pub fn compact_messages(
    messages: Vec<AgentMessage>,
    config: &ContextConfig,
    budget_state: &CompactionBudgetState,
) -> CompactionResult {
    let initial_snapshot = ContextSnapshot::new(&messages, config, budget_state);
    let ctx = PhaseContext {
        budget: BudgetTargets {
            max_tokens: initial_snapshot.budget,
            compact_target: initial_snapshot.compact_target,
        },
        bounds: RetentionBounds {
            keep_recent: config.keep_recent,
            keep_first: config.keep_first,
            max_messages: config.max_messages,
            message_limit_target_pct: config.message_limit_target_pct,
        },
        shrink: ShrinkSettings {
            tool_output_max_lines: config.tool_output_max_lines,
        },
        policy: CompactionPolicy::default(),
    };

    let before_message_count = initial_snapshot.message_count;
    let before_estimated_tokens = budget_state.estimated_tokens;
    let before_tool_details = collect_tool_details(&messages);
    let original_message_tokens = initial_snapshot.message_tokens;

    let mut runtime = CompactionRuntime {
        messages,
        current_tokens: original_message_tokens,
        estimated_tokens: before_estimated_tokens,
        original_message_tokens,
        original_estimated_tokens: before_estimated_tokens,
        original_image_count: initial_snapshot.image_count,
    };
    let mut all_actions = Vec::new();
    let mut level = 0;

    for strategy in default_levels() {
        let snapshot = ContextSnapshot::new(&runtime.messages, config, &runtime.budget_state());
        let pressure = PressureState::classify(&snapshot, config.system_prompt_tokens);
        let input = LevelInput {
            snapshot: &snapshot,
            pressure: &pressure,
            phase_context: &ctx,
            current_tokens: runtime.current_tokens,
        };

        if !strategy.should_run(&input) {
            continue;
        }

        let messages = mem::take(&mut runtime.messages);
        let result = strategy.run(messages, &input);
        if !result.actions.is_empty() {
            level = level.max(strategy.level());
        }

        let result = runtime.observe_result(result, &snapshot, config);
        all_actions.extend(result.actions);
    }

    let pre_sanitize_tokens = total_tokens(&runtime.messages);
    let messages = sanitize_tool_pairs(runtime.messages);
    let post_sanitize_tokens = total_tokens(&messages);
    let sanitize_removed = pre_sanitize_tokens.saturating_sub(post_sanitize_tokens);
    runtime.current_tokens = runtime.current_tokens.saturating_sub(sanitize_removed);
    runtime.current_tokens = runtime.current_tokens.max(post_sanitize_tokens);

    let after_message_count = messages.len();
    let after_message_tokens = runtime.current_tokens;
    let after_estimated_tokens = if after_message_tokens == original_message_tokens {
        before_estimated_tokens
    } else {
        before_estimated_tokens
            .saturating_sub(original_message_tokens.saturating_sub(after_message_tokens))
            .max(after_message_tokens)
    };
    let after_tool_details = collect_tool_details(&messages);

    let stats = build_stats(StatsInput {
        level,
        before_message_count,
        after_message_count,
        before_estimated_tokens,
        after_estimated_tokens,
        before_tool_details,
        after_tool_details,
        actions: all_actions,
    });

    CompactionResult { messages, stats }
}
