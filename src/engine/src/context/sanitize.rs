//! Normalize tool exchanges without changing persisted transcripts.
//!
//! Missing outcomes become explicit failed results, never invented successful
//! executions. Duplicate or displaced results cannot satisfy a later exchange.

use std::collections::HashSet;

use crate::types::*;

/// Complete missing results before the next user/assistant message or EOF.
/// Preserve assistant content (including signed thinking) and call order.
/// UI-only extensions do not interrupt an exchange. Failed/aborted assistant
/// turns are not replayed, nor are tool results belonging only to those turns.
pub fn sanitize_tool_pairs(messages: Vec<AgentMessage>) -> Vec<AgentMessage> {
    let mut result = Vec::with_capacity(messages.len());
    let mut pending: Vec<(String, String)> = Vec::new();
    let mut unanswered = HashSet::new();
    let mut timestamp = 0;

    for message in messages {
        match message {
            AgentMessage::Llm(Message::Assistant {
                mut content,
                stop_reason,
                model,
                provider,
                usage,
                timestamp: assistant_timestamp,
                error_message,
                response_id,
            }) => {
                complete_exchange(&mut result, &mut pending, &mut unanswered, timestamp);
                if matches!(stop_reason, StopReason::Error | StopReason::Aborted) {
                    continue;
                }
                content.retain(|block| match block {
                    Content::ToolCall { id, .. } => unanswered.insert(id.clone()),
                    _ => true,
                });
                pending = content
                    .iter()
                    .filter_map(|block| match block {
                        Content::ToolCall { id, name, .. } => Some((id.clone(), name.clone())),
                        _ => None,
                    })
                    .collect();
                timestamp = assistant_timestamp;
                result.push(AgentMessage::Llm(Message::Assistant {
                    content,
                    stop_reason,
                    model,
                    provider,
                    usage,
                    timestamp: assistant_timestamp,
                    error_message,
                    response_id,
                }));
            }
            AgentMessage::Llm(Message::ToolResult {
                ref tool_call_id, ..
            }) => {
                if unanswered.remove(tool_call_id) {
                    result.push(message);
                }
            }
            AgentMessage::Llm(Message::User { .. }) => {
                complete_exchange(&mut result, &mut pending, &mut unanswered, timestamp);
                result.push(message);
            }
            AgentMessage::Extension(_) => result.push(message),
        }
    }
    complete_exchange(&mut result, &mut pending, &mut unanswered, timestamp);
    result
}

fn complete_exchange(
    result: &mut Vec<AgentMessage>,
    pending: &mut Vec<(String, String)>,
    unanswered: &mut HashSet<String>,
    timestamp: u64,
) {
    for (id, name) in pending.drain(..) {
        if unanswered.remove(&id) {
            result.push(AgentMessage::Llm(Message::ToolResult {
                tool_call_id: id,
                tool_name: name,
                content: vec![Content::Text {
                    text: "No result provided".into(),
                }],
                is_error: true,
                // Stable across repeated normalization; this is a missing
                // historical outcome, not a tool execution happening now.
                timestamp,
                retention: Retention::Normal,
            }));
        }
    }
    unanswered.clear();
}
