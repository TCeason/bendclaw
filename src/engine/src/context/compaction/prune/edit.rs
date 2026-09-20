//! Applying verdicts to a message list: remove call/result pairs, truncate
//! results to a head. The only place prune changes history.

use std::collections::HashSet;

use super::state::text_of;
use crate::types::AgentMessage;
use crate::types::Content;
use crate::types::Message;

pub(super) fn apply(
    messages: Vec<AgentMessage>,
    remove: &HashSet<String>,
    truncate: &HashSet<String>,
    head_chars: usize,
) -> Vec<AgentMessage> {
    if remove.is_empty() && truncate.is_empty() {
        return messages;
    }
    messages
        .into_iter()
        .filter_map(|message| match message {
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
                let content: Vec<Content> = content
                    .into_iter()
                    .filter(|block| !matches!(block, Content::ToolCall { id, .. } if remove.contains(id)))
                    .collect();
                // A reply that only carried removed calls has nothing left to say.
                (!content.is_empty()).then(|| {
                    AgentMessage::Llm(Message::Assistant {
                        content,
                        stop_reason,
                        model,
                        provider,
                        usage,
                        timestamp,
                        error_message,
                        response_id,
                    })
                })
            }
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
                retention,
            }) => {
                if remove.contains(&tool_call_id) {
                    return None;
                }
                let content = if truncate.contains(&tool_call_id) {
                    truncate_result(content, head_chars)
                } else {
                    content
                };
                Some(AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content,
                    is_error,
                    timestamp,
                    retention,
                }))
            }
            other => Some(other),
        })
        .collect()
}

fn truncate_result(content: Vec<Content>, head_chars: usize) -> Vec<Content> {
    let full = text_of(&content);
    let total = full.chars().count();
    if total <= head_chars {
        return content;
    }
    let head: String = full.chars().take(head_chars).collect();
    vec![Content::Text {
        text: format!(
            "{head}\n[… {} more chars pruned: no longer needed for the current task …]",
            total - head_chars
        ),
    }]
}
