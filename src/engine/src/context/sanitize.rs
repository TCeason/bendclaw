//! Tool pair sanitization — ensures every tool call has a matching result and vice-versa.

use std::collections::HashSet;

use crate::types::*;

fn has_content(content: &[Content]) -> bool {
    content.iter().any(|c| match c {
        Content::Text { text } => !text.is_empty(),
        Content::Thinking { thinking, .. } => !thinking.is_empty(),
        Content::Image { .. } | Content::ToolCall { .. } => true,
    })
}

/// Ensure every tool call has a matching tool result and vice-versa.
/// Orphaned entries are removed.
pub fn sanitize_tool_pairs(messages: Vec<AgentMessage>) -> Vec<AgentMessage> {
    let mut call_ids: HashSet<String> = HashSet::new();
    let mut result_ids: HashSet<String> = HashSet::new();

    for msg in &messages {
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                for c in content {
                    if let Content::ToolCall { id, .. } = c {
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

    let orphan_calls: HashSet<String> = call_ids.difference(&result_ids).cloned().collect();
    let orphan_results: HashSet<String> = result_ids.difference(&call_ids).cloned().collect();

    if orphan_calls.is_empty() && orphan_results.is_empty() {
        return messages;
    }

    messages
        .into_iter()
        .filter_map(|msg| match msg {
            AgentMessage::Llm(Message::ToolResult {
                ref tool_call_id, ..
            }) if orphan_results.contains(tool_call_id) => None,

            AgentMessage::Llm(Message::Assistant {
                content,
                stop_reason,
                model,
                provider,
                usage,
                timestamp,
                error_message,
                response_id,
            }) => {
                let filtered: Vec<Content> = content
                    .into_iter()
                    .filter(
                        |c| !matches!(c, Content::ToolCall { id, .. } if orphan_calls.contains(id)),
                    )
                    .collect();
                if has_content(&filtered) {
                    Some(AgentMessage::Llm(Message::Assistant {
                        content: filtered,
                        stop_reason,
                        model,
                        provider,
                        usage,
                        timestamp,
                        error_message,
                        response_id,
                    }))
                } else {
                    None
                }
            }

            other => Some(other),
        })
        .collect()
}
