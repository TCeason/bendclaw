use std::collections::HashSet;

use evotengine::types::*;

/// Assert the message list contains no orphan tool calls/results.
pub fn assert_no_orphan_tool_pairs(messages: &[AgentMessage]) {
    let mut call_ids = HashSet::new();
    let mut result_ids = HashSet::new();

    for msg in messages {
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                for block in content {
                    if let Content::ToolCall { id, .. } = block {
                        call_ids.insert(id.clone());
                    }
                }
            }
            AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) => {
                result_ids.insert(tool_call_id.clone());
            }
            _ => {}
        }
    }

    assert_eq!(
        call_ids, result_ids,
        "tool call/result ids differ: calls={call_ids:?}, results={result_ids:?}"
    );
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
