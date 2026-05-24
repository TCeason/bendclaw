//! Pipeline runner: executes passes in order, tracks state between them.

use super::accounting::build_stats;
use super::accounting::collect_tool_details;
use super::accounting::StatsInput;
use super::config::CompactionConfig;
use super::pass::Pass;
use super::pass::PassContext;
use super::passes::collapse;
use super::passes::evict;
use super::passes::microcompact;
use super::passes::reclaim;
use super::passes::shrink;
use super::pressure::Pressure;
use super::sanitize::sanitize_tool_pairs;
use super::types::CompactionAction;
use super::types::CompactionResult;
use crate::context::tokens::total_tokens;
use crate::types::AgentMessage;

fn default_pipeline() -> Vec<Box<dyn Pass>> {
    vec![
        Box::new(reclaim::Reclaim),
        Box::new(shrink::Shrink),
        Box::new(microcompact::Microcompact),
        Box::new(collapse::Collapse),
        Box::new(evict::Evict),
    ]
}

/// Run the full compaction pipeline on messages.
pub fn run(
    messages: Vec<AgentMessage>,
    config: &CompactionConfig,
    estimated_tokens: usize,
) -> CompactionResult {
    let passes = default_pipeline();
    let before_message_count = messages.len();
    let before_tool_details = collect_tool_details(&messages);

    let mut current_messages = messages;
    let before_message_tokens = total_tokens(&current_messages);
    let before_estimated_tokens = before_message_tokens.max(estimated_tokens);
    let mut current_estimated_tokens = before_estimated_tokens;
    let mut all_actions: Vec<CompactionAction> = Vec::new();
    let mut max_level: u8 = 0;

    for pass in &passes {
        let ctx = PassContext {
            config,
            pressure: Pressure::from_messages(&current_messages, config, current_estimated_tokens),
        };

        if !pass.should_run(&ctx) {
            continue;
        }

        let result = pass.run(current_messages, &ctx);
        let saved: usize = result
            .actions
            .iter()
            .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
            .sum();
        current_estimated_tokens = current_estimated_tokens.saturating_sub(saved);
        let had_actions = !result.actions.is_empty();
        if had_actions {
            max_level = max_level.max(pass.level().as_u8());
        }

        all_actions.extend(result.actions);
        current_messages = result.messages;
    }

    // Post-step: sanitize orphaned tool pairs (always runs, not a Pass)
    current_messages = sanitize_tool_pairs(current_messages);

    let after_message_count = current_messages.len();
    let after_tool_details = collect_tool_details(&current_messages);
    let after_message_tokens = total_tokens(&current_messages);

    // after_estimated_tokens must satisfy:
    //   1. <= before_estimated_tokens (compaction never inflates)
    //   2. >= total_tokens(result.messages) (can't be less than actual)
    let savings = before_message_tokens.saturating_sub(after_message_tokens);
    let after_estimated_tokens = before_estimated_tokens
        .saturating_sub(savings)
        .max(after_message_tokens);

    let stats = build_stats(StatsInput {
        level: max_level,
        before_message_count,
        after_message_count,
        before_estimated_tokens,
        after_estimated_tokens,
        before_tool_details,
        after_tool_details,
        actions: all_actions,
    });

    CompactionResult {
        messages: current_messages,
        stats,
    }
}
