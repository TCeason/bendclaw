//! Context compaction: shrink context when approaching token budget.

use tokio::sync::mpsc;

use super::config::AgentLoopConfig;
use crate::context::CompactionBudgetState;
use crate::context::CompactionStrategy;
use crate::context::ContextTracker;
use crate::context::DefaultCompaction;
use crate::types::*;

/// Run context compaction if configured.
pub(super) fn compact_context(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    context_tracker: &mut ContextTracker,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) {
    let strategy: &dyn CompactionStrategy = config
        .compaction_strategy
        .as_deref()
        .unwrap_or(&DefaultCompaction);

    let ctx_config = match config.context_config {
        Some(ref c) => c,
        None => return,
    };

    let original_count = context.messages.len();
    let budget = context_tracker.budget_snapshot(&context.messages, Some(ctx_config));
    let pre_stats = crate::context::compute_call_stats_from_agent_messages(&context.messages);

    let budget_state = CompactionBudgetState::from_tracker(context_tracker, &context.messages);
    let compact_budget = crate::context::ContextBudgetSnapshot {
        estimated_tokens: budget_state.estimated_tokens,
        budget_tokens: budget.budget_tokens,
        system_prompt_tokens: budget.system_prompt_tokens,
        tool_definition_tokens: budget.tool_definition_tokens,
        context_window: budget.context_window,
    };
    let result = strategy.compact(
        std::mem::take(&mut context.messages),
        ctx_config,
        &budget_state,
    );
    context.messages = result.messages;

    tx.send(AgentEvent::ContextCompactionStart {
        message_count: original_count,
        budget: compact_budget,
        message_stats: pre_stats,
    })
    .ok();

    // Adjust baseline by what compaction actually saved, rather than resetting
    // it entirely. A full reset would fall back to chars/4 which severely
    // underestimates images. Keeping the adjusted baseline lets the next
    // compaction check see the real cost of remaining content (especially images).
    if result.stats.level > 0 {
        let saved = result
            .stats
            .before_estimated_tokens
            .saturating_sub(result.stats.after_estimated_tokens);
        context_tracker.record_compaction_savings(saved, context.messages.len());
    }

    tx.send(AgentEvent::ContextCompactionEnd {
        stats: result.stats,
        messages: context.messages.clone(),
        context_window: budget.context_window,
    })
    .ok();
}
