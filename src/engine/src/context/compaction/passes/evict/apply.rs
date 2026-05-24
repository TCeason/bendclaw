use super::EvictionPlan;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::types::CompactionAction;
use crate::context::compaction::types::CompactionMethod;
use crate::context::tokens::message_tokens;
use crate::types::AgentMessage;

pub(super) fn apply_plan(messages: Vec<AgentMessage>, plan: EvictionPlan) -> PassResult {
    let span = plan.span;
    if span.start >= span.end || span.end > messages.len() {
        return no_op(messages);
    }

    let removed = span.len();
    let dropped_tokens: usize = messages[span.start..span.end]
        .iter()
        .map(message_tokens)
        .sum();

    let mut result =
        Vec::with_capacity(messages.len() - removed + usize::from(plan.marker.is_some()));
    result.extend_from_slice(&messages[..span.start]);
    if let Some(marker_msg) = plan.marker {
        result.push(marker_msg);
    }
    result.extend_from_slice(&messages[span.end..]);

    PassResult {
        messages: result,
        actions: vec![CompactionAction {
            index: span.start,
            tool_name: "messages".into(),
            method: CompactionMethod::MessagesEvicted,
            before_tokens: dropped_tokens,
            after_tokens: plan.after_tokens,
            end_index: Some(span.end - 1),
            related_count: Some(removed),
        }],
    }
}

pub(super) fn no_op(messages: Vec<AgentMessage>) -> PassResult {
    PassResult {
        messages,
        actions: vec![],
    }
}
