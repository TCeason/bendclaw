//! Tool call / tool result pair sanitization.

use std::collections::HashSet;

use crate::context::tokens::message_tokens;
use crate::context::tokens::total_tokens;
use crate::types::*;

fn has_content(content: &[Content]) -> bool {
    content.iter().any(|c| match c {
        Content::Text { text } => !text.is_empty(),
        Content::Thinking { thinking, .. } => !thinking.is_empty(),
        Content::Image { .. } | Content::ToolCall { .. } => true,
    })
}

/// Sanitize tool call / tool result pairing in a message list.
///
/// Ensures every assistant `Content::ToolCall` has a matching `Message::ToolResult`
/// and vice-versa. Orphaned entries are removed so the message list stays valid
/// for providers (e.g. OpenAI) that enforce strict pairing.
///
/// Fast path: when no orphans exist the original `Vec` is returned untouched.
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

    let before_tokens = total_tokens(&messages);
    let filtered: Vec<AgentMessage> = messages
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

            AgentMessage::Llm(Message::User { content, timestamp }) => {
                if has_content(&content) {
                    Some(AgentMessage::Llm(Message::User { content, timestamp }))
                } else {
                    None
                }
            }

            other => Some(other),
        })
        .collect();

    // Avoid turning a tiny/empty orphan into a larger synthetic message. When
    // there was real content, keep a minimal marker so compaction does not
    // erase a non-empty conversation completely.
    if filtered.is_empty() {
        let marker = super::marker::build_fallback_marker();
        if message_tokens(&marker) <= before_tokens {
            return vec![marker];
        }
        return Vec::new();
    }

    filtered
}
