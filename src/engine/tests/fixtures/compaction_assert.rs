use std::collections::HashSet;

use evotengine::types::*;

/// Check exchange-local adjacency and uniqueness, not global ID membership.
/// Reusing an ID in a later exchange must not hide a missing earlier result.
pub fn assert_no_orphan_tool_pairs(messages: &[AgentMessage]) {
    let mut pending = HashSet::new();
    for msg in messages {
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                assert!(
                    pending.is_empty(),
                    "unanswered calls before assistant: {pending:?}"
                );
                for block in content {
                    if let Content::ToolCall { id, .. } = block {
                        assert!(pending.insert(id.clone()), "duplicate tool call: {id}");
                    }
                }
            }
            AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) => {
                assert!(
                    pending.remove(tool_call_id),
                    "displaced/duplicate tool result: {tool_call_id}"
                );
            }
            AgentMessage::Llm(Message::User { .. }) => {
                assert!(
                    pending.is_empty(),
                    "unanswered calls before user: {pending:?}"
                );
            }
            AgentMessage::Extension(_) => {}
        }
    }
    assert!(pending.is_empty(), "unanswered trailing calls: {pending:?}");
}

/// Assert a message list has the exact structural pattern expected by the DSL.
///
/// Supported symbols mirror `message_dsl`:
/// - `u`: user message
/// - `a`: assistant text-only message
/// - `t`: assistant containing at least one tool call
/// - `r`: tool result
pub fn assert_pattern(messages: &[AgentMessage], expected: &str) {
    let actual: String = messages.iter().map(message_symbol).collect();
    let normalized_expected: String = expected.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(actual, normalized_expected);
}

pub fn count_user_markers(messages: &[AgentMessage]) -> usize {
    messages
        .iter()
        .filter(|msg| match msg {
            AgentMessage::Llm(Message::User { content, .. }) => content.iter().any(
                |c| matches!(c, Content::Text { text } if text.contains("[Context compacted")),
            ),
            _ => false,
        })
        .count()
}

fn message_symbol(message: &AgentMessage) -> char {
    match message {
        AgentMessage::Llm(Message::User { .. }) => 'u',
        AgentMessage::Llm(Message::ToolResult { .. }) => 'r',
        AgentMessage::Llm(Message::Assistant { content, .. }) => {
            if content
                .iter()
                .any(|block| matches!(block, Content::ToolCall { .. }))
            {
                't'
            } else {
                'a'
            }
        }
        AgentMessage::Extension(_) => 'x',
    }
}
