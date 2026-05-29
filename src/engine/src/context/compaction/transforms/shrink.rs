//! Shrink transform — truncate oversized tool results in the retained zone.

use std::ops::Range;

use crate::context::compaction::config::CompactionConfig;
use crate::context::tokens::content_tokens;
use crate::types::*;

/// Run shrink on messages within the retained range only.
/// Returns transformed messages and count of shrunk results.
pub fn run(
    mut messages: Vec<AgentMessage>,
    retained_range: &Range<usize>,
    config: &CompactionConfig,
) -> (Vec<AgentMessage>, usize) {
    let mut shrunk_count = 0usize;

    for idx in retained_range.clone() {
        if idx >= messages.len() {
            break;
        }

        if let AgentMessage::Llm(Message::ToolResult {
            ref content,
            ref tool_name,
            ..
        }) = messages[idx]
        {
            let tokens = content_tokens(content);
            if tokens <= config.max_tool_result_tokens {
                continue;
            }

            // Truncate: head + tail strategy
            let text = extract_text(content);
            if text.is_empty() {
                continue;
            }

            let truncated = truncate_head_tail(&text, config.tool_output_max_lines, tool_name);
            if let AgentMessage::Llm(Message::ToolResult {
                ref tool_call_id,
                ref tool_name,
                ref is_error,
                ref timestamp,
                ref retention,
                ..
            }) = messages[idx]
            {
                let new_content = vec![Content::Text { text: truncated }];
                let new_tokens = content_tokens(&new_content);
                if new_tokens < tokens {
                    shrunk_count += 1;
                    messages[idx] = AgentMessage::Llm(Message::ToolResult {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        content: new_content,
                        is_error: *is_error,
                        timestamp: *timestamp,
                        retention: *retention,
                    });
                }
            }
        }
    }

    (messages, shrunk_count)
}

/// Extract all text content from a content slice.
fn extract_text(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Truncate text keeping head and tail lines, with a marker in between.
pub fn truncate_head_tail(text: &str, max_lines: usize, tool_name: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }

    let head_lines = max_lines * 2 / 3;
    let tail_lines = max_lines - head_lines;
    let omitted = lines.len() - head_lines - tail_lines;

    let head: String = lines[..head_lines].join("\n");
    let tail: String = lines[lines.len() - tail_lines..].join("\n");

    format!("{head}\n\n[… {omitted} lines omitted from {tool_name} output …]\n\n{tail}")
}
