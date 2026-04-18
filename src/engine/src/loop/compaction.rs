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
    let original_tokens = context_tracker.estimate_context_tokens(&context.messages);

    let budget = ctx_config
        .max_context_tokens
        .saturating_sub(ctx_config.system_prompt_tokens);

    let budget_state = CompactionBudgetState {
        estimated_tokens: original_tokens,
    };
    let result = strategy.compact(
        std::mem::take(&mut context.messages),
        ctx_config,
        &budget_state,
    );
    let did_work = result.stats.level > 0 || result.stats.current_run_cleared > 0;
    context.messages = result.messages;

    let compact_stats = crate::context::compute_call_stats_from_agent_messages(&context.messages);
    tx.send(AgentEvent::ContextCompactionStart {
        message_count: original_count,
        estimated_tokens: original_tokens,
        budget_tokens: budget,
        system_prompt_tokens: ctx_config.system_prompt_tokens,
        context_window: ctx_config.max_context_tokens,
        message_stats: compact_stats,
    })
    .ok();

    if did_work {
        context_tracker.reset();
    }

    tx.send(AgentEvent::ContextCompactionEnd {
        stats: result.stats,
        messages: context.messages.clone(),
    })
    .ok();
}

/// Force compaction for error recovery.
///
/// Removes the last error message from both `context.messages` and
/// `new_messages`, then compacts. Returns true if recovery was performed.
/// All-or-nothing: if conditions aren't met, nothing is touched.
pub(super) fn compact_for_recovery(
    context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    context_tracker: &mut ContextTracker,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> bool {
    let strategy: &dyn CompactionStrategy = config
        .compaction_strategy
        .as_deref()
        .unwrap_or(&DefaultCompaction);

    let ctx_config = match config.context_config {
        Some(ref c) => c,
        None => return false,
    };

    let budget = ctx_config
        .max_context_tokens
        .saturating_sub(ctx_config.system_prompt_tokens);
    let current_tokens = context_tracker.estimate_context_tokens(&context.messages);

    if current_tokens <= budget / 2 {
        return false;
    }

    // Remove the error message before compaction so it doesn't pollute the result
    context.messages.pop();
    new_messages.pop();

    let budget_state = CompactionBudgetState {
        estimated_tokens: current_tokens,
    };
    let compact_result = strategy.compact(
        std::mem::take(&mut context.messages),
        ctx_config,
        &budget_state,
    );
    context.messages = compact_result.messages;
    context_tracker.reset();

    tx.send(AgentEvent::ContextCompactionStart {
        message_count: compact_result.stats.before_message_count,
        estimated_tokens: compact_result.stats.before_estimated_tokens,
        budget_tokens: budget,
        system_prompt_tokens: ctx_config.system_prompt_tokens,
        context_window: ctx_config.max_context_tokens,
        message_stats: crate::context::compute_call_stats_from_agent_messages(&context.messages),
    })
    .ok();
    tx.send(AgentEvent::ContextCompactionEnd {
        stats: compact_result.stats,
        messages: context.messages.clone(),
    })
    .ok();

    true
}
