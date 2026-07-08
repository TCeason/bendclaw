//! Tool pair sanitization — ensures every tool call has a matching result and vice-versa.
//!
//! Anthropic-compatible providers require *adjacency*: a `tool_result` must
//! correspond to a `tool_use` in the immediately preceding message. Loaded
//! history can violate this — e.g. a run interrupted mid-tool-execution may have
//! persisted a `tool_result` without its `tool_use`, or with another message
//! between them. Such history is rejected with HTTP 400, so a global
//! id-membership check is insufficient; placement must be validated.

use std::collections::HashSet;

use crate::types::*;

/// Remove repeated `ToolCall` blocks (same id) within one assistant message,
/// keeping the first occurrence.
fn dedup_tool_call_ids(msg: AgentMessage) -> AgentMessage {
    let AgentMessage::Llm(Message::Assistant {
        content,
        stop_reason,
        model,
        provider,
        usage,
        timestamp,
        error_message,
        response_id,
    }) = msg
    else {
        return msg;
    };
    let mut seen: HashSet<String> = HashSet::new();
    let content = content
        .into_iter()
        .filter(|c| match c {
            Content::ToolCall { id, .. } => seen.insert(id.clone()),
            _ => true,
        })
        .collect();
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
}

fn has_content(content: &[Content]) -> bool {
    content.iter().any(|c| match c {
        Content::Text { text } => !text.is_empty(),
        Content::Thinking { thinking, .. } => !thinking.is_empty(),
        Content::Image { .. } | Content::ToolCall { .. } => true,
    })
}

/// Remove tool calls and tool results that lack an adjacent matching partner.
///
/// A `tool_result` is valid only when the nearest preceding LLM message is an
/// assistant message that still has an unconsumed `tool_use` with the same id.
/// User messages reset the pairing; extension messages are skipped because they
/// are dropped before the request is built. Each id is consumed once, so a
/// duplicated result becomes an orphan. Orphaned `tool_use` blocks and orphaned
/// results are removed; matched pairs pass through untouched.
pub fn sanitize_tool_pairs(messages: Vec<AgentMessage>) -> Vec<AgentMessage> {
    // Anthropic rejects duplicate tool_use ids ("`tool_use` ids must be
    // unique"); persisted history may contain them from a past decoder bug.
    // Keep the first occurrence; extra results are dropped by pairing below.
    let messages: Vec<AgentMessage> = messages.into_iter().map(dedup_tool_call_ids).collect();
    let len = messages.len();

    // Pass 1: classify by adjacency, tracking orphans per message instance.
    //
    // `available` holds the unconsumed tool_use ids of the current pending
    // assistant message (`pending_idx`). A contiguous run of results draws from
    // it; a user/assistant message or end of history flushes whatever remains as
    // orphan calls of that assistant. Tracking by index (not by id) keeps
    // classification correct even if an id is reused across turns.
    let mut available: HashSet<String> = HashSet::new();
    let mut pending_idx: Option<usize> = None;
    let mut orphan_calls_at: Vec<HashSet<String>> = vec![HashSet::new(); len];
    let mut result_valid: Vec<bool> = Vec::with_capacity(len);

    for (idx, msg) in messages.iter().enumerate() {
        match msg {
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                if let Some(prev) = pending_idx.take() {
                    orphan_calls_at[prev] = std::mem::take(&mut available);
                }
                available = content
                    .iter()
                    .filter_map(|c| match c {
                        Content::ToolCall { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                pending_idx = if available.is_empty() {
                    None
                } else {
                    Some(idx)
                };
                result_valid.push(false);
            }
            AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) => {
                result_valid.push(available.remove(tool_call_id));
            }
            AgentMessage::Llm(Message::User { .. }) => {
                if let Some(prev) = pending_idx.take() {
                    orphan_calls_at[prev] = std::mem::take(&mut available);
                }
                result_valid.push(false);
            }
            AgentMessage::Extension(_) => {
                result_valid.push(false);
            }
        }
    }
    if let Some(prev) = pending_idx {
        orphan_calls_at[prev] = available;
    }

    let has_orphan_call = orphan_calls_at.iter().any(|ids| !ids.is_empty());
    let has_invalid_result = result_valid.iter().zip(&messages).any(|(valid, msg)| {
        matches!(msg, AgentMessage::Llm(Message::ToolResult { .. })) && !*valid
    });

    if !has_orphan_call && !has_invalid_result {
        return messages;
    }

    // Pass 2: drop misplaced results and orphaned tool calls.
    messages
        .into_iter()
        .zip(result_valid)
        .enumerate()
        .filter_map(|(idx, (msg, valid_result))| match msg {
            AgentMessage::Llm(Message::ToolResult { .. }) if !valid_result => None,

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
                let orphans = &orphan_calls_at[idx];
                let filtered: Vec<Content> = content
                    .into_iter()
                    .filter(|c| !matches!(c, Content::ToolCall { id, .. } if orphans.contains(id)))
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
