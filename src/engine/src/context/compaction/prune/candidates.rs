//! Which tool calls may be judged: every call paired with its result, minus
//! the first message and the pinned tail.

use std::collections::HashMap;

use crate::context::tokens::content_tokens;
use crate::types::AgentMessage;
use crate::types::Content;
use crate::types::Message;

#[derive(Debug, Clone)]
pub(super) struct Candidate {
    pub call_id: String,
    pub tool_name: String,
    /// Compact JSON of the call arguments.
    pub arguments: String,
    /// "ok, 1200 chars (omitted)": names the result without quoting it.
    pub result_note: String,
    /// Index of the assistant message holding the call, and of its result.
    pub call_index: usize,
    pub result_index: Option<usize>,
    /// Tokens freed by removing the pair (call block + result message).
    pub pair_tokens: usize,
    /// Tokens freed by truncating the result to its head.
    pub truncation_tokens: usize,
}

pub(super) fn collect(messages: &[AgentMessage], preserve_recent: usize) -> Vec<Candidate> {
    let pinned_from = messages.len().saturating_sub(preserve_recent);
    let mut results: HashMap<&str, (usize, &Message)> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if let AgentMessage::Llm(result @ Message::ToolResult { tool_call_id, .. }) = message {
            results.insert(tool_call_id.as_str(), (index, result));
        }
    }
    let mut candidates = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if index == 0 || index >= pinned_from {
            continue;
        }
        let AgentMessage::Llm(Message::Assistant { content, .. }) = message else {
            continue;
        };
        for block in content {
            let Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } = block
            else {
                continue;
            };
            let result = results.get(id.as_str()).copied();
            if result.is_some_and(|(at, _)| at >= pinned_from) {
                continue; // its result is pinned: leave the pair alone
            }
            let result_tokens = result
                .map(|(_, r)| match r {
                    Message::ToolResult { content, .. } => content_tokens(content),
                    _ => 0,
                })
                .unwrap_or(0);
            let call_tokens = content_tokens(std::slice::from_ref(block));
            candidates.push(Candidate {
                call_id: id.clone(),
                tool_name: name.clone(),
                arguments: compact_json(arguments),
                result_note: result
                    .map(|(_, r)| result_note(r))
                    .unwrap_or_else(|| "no result".into()),
                call_index: index,
                result_index: result.map(|(at, _)| at),
                pair_tokens: call_tokens + result_tokens,
                truncation_tokens: result_tokens.saturating_sub(result_tokens.min(100)),
            });
        }
    }
    candidates
}

fn result_note(message: &Message) -> String {
    let Message::ToolResult {
        content, is_error, ..
    } = message
    else {
        return String::new();
    };
    let chars: usize = content.iter().map(block_chars).sum();
    format!(
        "{}, {chars} chars (omitted)",
        if *is_error { "error" } else { "ok" }
    )
}

fn block_chars(block: &Content) -> usize {
    match block {
        Content::Text { text } => text.chars().count(),
        Content::Thinking { thinking, .. } => thinking.chars().count(),
        Content::ToolCall { arguments, .. } => arguments.to_string().len(),
        Content::Image { .. } => 0,
    }
}

pub(super) fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}
