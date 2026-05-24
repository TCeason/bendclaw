use super::bounds::is_tool_call_assistant;
use super::bounds::is_turn_start;
use super::Span;
use crate::types::*;

pub(super) fn safe_message_windows(
    messages: &[AgentMessage],
    bounds: Span,
    need_remove: usize,
) -> Vec<Span> {
    let units = safe_message_units(messages, bounds);
    let mut spans = Vec::new();
    for start in 0..units.len() {
        for end in (start + 1)..=units.len() {
            let span = Span {
                start: units[start].start,
                end: units[end - 1].end,
            };
            if span.len() >= need_remove {
                spans.push(span);
            }
        }
    }
    spans
}

fn safe_message_units(messages: &[AgentMessage], bounds: Span) -> Vec<Span> {
    let mut units = Vec::new();
    let mut idx = bounds.start;
    while idx < bounds.end {
        if is_tool_call_assistant(&messages[idx]) {
            let mut end = idx + 1;
            while end < bounds.end
                && matches!(messages[end], AgentMessage::Llm(Message::ToolResult { .. }))
            {
                end += 1;
            }
            units.push(Span { start: idx, end });
            idx = end;
        } else if is_turn_start(&messages[idx]) {
            let start = idx;
            idx += 1;
            while idx < bounds.end && !is_turn_start(&messages[idx]) {
                if is_tool_call_assistant(&messages[idx]) {
                    break;
                }
                idx += 1;
            }
            if start < idx {
                units.push(Span { start, end: idx });
            }
        } else {
            units.push(Span {
                start: idx,
                end: idx + 1,
            });
            idx += 1;
        }
    }
    units
}

pub(super) fn span_score(messages: &[AgentMessage], span: Span) -> usize {
    messages[span.start..span.end]
        .iter()
        .map(message_drop_cost)
        .sum::<usize>()
}

fn message_drop_cost(message: &AgentMessage) -> usize {
    match message {
        AgentMessage::Llm(Message::User { .. }) => 1_000,
        AgentMessage::Llm(Message::Assistant { content, .. }) => {
            if content
                .iter()
                .any(|c| matches!(c, Content::ToolCall { .. }))
            {
                0
            } else {
                10
            }
        }
        AgentMessage::Llm(Message::ToolResult { .. }) => 0,
        AgentMessage::Extension(_) => 10,
    }
}
