use super::Span;
use crate::context::compaction::config::CompactionConfig;
use crate::types::*;

pub(super) fn token_eviction_bounds(
    messages: &[AgentMessage],
    config: &CompactionConfig,
) -> Option<Span> {
    let start = protected_prefix_end(messages, config.keep_first.min(messages.len()));
    let recent_floor = messages.len().saturating_sub(config.keep_recent);
    let end = recent_floor.max(messages.len().saturating_sub(1));
    (start < end).then_some(Span { start, end })
}

pub(super) fn eviction_bounds(
    messages: &[AgentMessage],
    config: &CompactionConfig,
) -> Option<Span> {
    let start = protected_prefix_end(messages, config.keep_first.min(messages.len()));
    let end = protected_suffix_start(messages, messages.len().saturating_sub(config.keep_recent));
    (start < end).then_some(Span { start, end })
}

fn protected_suffix_start(messages: &[AgentMessage], recent_floor: usize) -> usize {
    let mut start = recent_floor.min(messages.len());
    if start == messages.len() {
        return start;
    }
    while start > 0 && !is_turn_start(&messages[start]) {
        start -= 1;
    }
    start
}

fn protected_prefix_end(messages: &[AgentMessage], keep_first: usize) -> usize {
    let mut end = keep_first.min(messages.len());
    while end < messages.len() {
        match &messages[end] {
            AgentMessage::Llm(Message::Assistant { content, .. })
                if end > 0
                    && is_turn_start(&messages[end - 1])
                    && !content
                        .iter()
                        .any(|c| matches!(c, Content::ToolCall { .. })) =>
            {
                end += 1;
            }
            AgentMessage::Llm(Message::ToolResult { .. })
                if end > 0 && is_tool_call_assistant(&messages[end - 1]) =>
            {
                end += 1;
            }
            _ => break,
        }
    }
    end
}

pub(super) fn is_turn_start(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::Llm(Message::User { .. }))
}

pub(super) fn is_tool_call_assistant(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::Llm(Message::Assistant { content, .. }) if content.iter().any(|c| matches!(c, Content::ToolCall { .. })))
}
