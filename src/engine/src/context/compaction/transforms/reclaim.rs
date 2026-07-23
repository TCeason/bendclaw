//! Reclaim transform — always-on, lossless cleanup.
//!
//! Clears expired `Retention::CurrentRun` tool results once their run is over
//! (a later user message exists), replacing the content with a short marker.

use crate::context::tokens::content_tokens;
use crate::types::*;

/// Run reclaim on all messages. Returns transformed messages and the number of
/// `CurrentRun` tool results that were cleared.
pub fn run(messages: Vec<AgentMessage>) -> (Vec<AgentMessage>, usize) {
    clear_current_run(messages)
}

fn clear_current_run(messages: Vec<AgentMessage>) -> (Vec<AgentMessage>, usize) {
    // Mark which messages have a user message after them.
    let mut has_user_after = vec![false; messages.len()];
    let mut seen_user = false;
    for i in (0..messages.len()).rev() {
        has_user_after[i] = seen_user;
        if matches!(&messages[i], AgentMessage::Llm(Message::User { .. })) {
            seen_user = true;
        }
    }

    let mut reclaimed = 0usize;
    let result = messages
        .into_iter()
        .enumerate()
        .map(|(idx, msg)| {
            let should_clear = matches!(
                &msg,
                AgentMessage::Llm(Message::ToolResult {
                    retention: Retention::CurrentRun,
                    ..
                })
            ) && has_user_after[idx];

            if !should_clear {
                return msg;
            }

            if let AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
                retention,
            }) = msg
            {
                let before_tokens = content_tokens(&content);
                let marker = vec![Content::Text {
                    text: format!("[{tool_name} result cleared after use]"),
                }];
                let marker_tokens = content_tokens(&marker);
                let replacement = if marker_tokens < before_tokens {
                    marker
                } else {
                    Vec::new()
                };

                reclaimed += 1;

                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content: replacement,
                    is_error,
                    timestamp,
                    retention,
                })
            } else {
                msg
            }
        })
        .collect();

    (result, reclaimed)
}
