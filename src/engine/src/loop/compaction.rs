//! Context compaction: shrink context when approaching token budget.

use tokio::sync::mpsc;

use super::config::AgentLoopConfig;
use crate::context::compact_messages;
use crate::context::CompactionBudgetState;
use crate::context::ContextTracker;
use crate::types::*;

/// Run context compaction if configured.
pub(super) fn compact_context(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    context_tracker: &mut ContextTracker,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) {
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
    let result = compact_messages(
        std::mem::take(&mut context.messages),
        ctx_config,
        &budget_state,
    );
    context.messages = result.messages;

    tx.send(AgentEvent::ContextCompactionStart {
        message_count: original_count,
        budget: compact_budget.clone(),
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

        // Inject current task list after compaction to prevent context loss.
        if let Some(note) = crate::tools::format_todo_for_compaction(&context.todo_state) {
            context
                .messages
                .push(AgentMessage::Llm(Message::system_reminder(note)));
        }
    }

    // Re-estimate tokens using the tracker so the ✓ line is consistent with
    // the ● line (both use tracker-based estimates that include images).
    let mut stats = result.stats;
    stats.before_estimated_tokens = compact_budget.estimated_tokens;
    stats.after_estimated_tokens = context_tracker.estimate_context_tokens(&context.messages);

    tx.send(AgentEvent::ContextCompactionEnd {
        stats,
        messages: context.messages.clone(),
        context_window: budget.context_window,
    })
    .ok();
}
