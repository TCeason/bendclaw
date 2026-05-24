//! Evict pass — remove stale spans only when smaller passes cannot fit context.

use super::apply;
use super::planner;
use crate::context::compaction::config::CompactionConfig;
use crate::context::compaction::pass::Pass;
use crate::context::compaction::pass::PassContext;
use crate::context::compaction::pass::PassLevel;
use crate::context::compaction::pass::PassResult;
use crate::types::AgentMessage;

pub struct Evict;

impl Pass for Evict {
    fn level(&self) -> PassLevel {
        PassLevel::Evict
    }

    fn should_run(&self, ctx: &PassContext<'_>) -> bool {
        ctx.pressure.message_tokens > ctx.config.budget_tokens
            || (ctx.config.max_messages > 0 && ctx.pressure.message_count > ctx.config.max_messages)
    }

    fn run(&self, messages: Vec<AgentMessage>, ctx: &PassContext<'_>) -> PassResult {
        let over_tokens = ctx.pressure.message_tokens > ctx.config.budget_tokens;
        let over_messages = ctx.config.max_messages > 0 && messages.len() > ctx.config.max_messages;

        if over_tokens {
            if let Some(plan) =
                planner::token_plan(&messages, ctx.config, ctx.pressure.message_tokens)
            {
                return apply::apply_plan(messages, plan);
            }
        }

        if over_messages {
            let target_len = message_target(ctx.config);
            if messages.len() <= target_len {
                return apply::no_op(messages);
            }
            if let Some(plan) = planner::message_limit_plan(&messages, ctx.config, target_len) {
                return apply::apply_plan(messages, plan);
            }
        }

        apply::no_op(messages)
    }
}

fn message_target(config: &CompactionConfig) -> usize {
    let pct_target = config.max_messages * config.message_limit_target_pct as usize / 100;
    let minimum = config
        .keep_first
        .saturating_add(config.keep_recent)
        .saturating_add(1);
    pct_target.max(minimum).min(config.max_messages)
}
