//! Text truncation utilities.

use std::collections::HashMap;

use super::outline::try_outline;
use crate::context::tokens::content_tokens;
use crate::types::*;

/// Truncate text keeping head and tail with a marker in between.
pub fn truncate_text_head_tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return truncate_single_block(text, max_lines);
    }

    let head_count = (max_lines * 3) / 5; // 60% head
    let tail_count = max_lines - head_count;
    let omitted = lines.len() - head_count - tail_count;

    let mut result = String::new();
    for line in &lines[..head_count] {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str(&format!("\n\n[... {} lines truncated ...]\n\n", omitted));
    for (i, line) in lines[lines.len() - tail_count..].iter().enumerate() {
        result.push_str(line);
        if i < tail_count - 1 {
            result.push('\n');
        }
    }
    result
}

fn truncate_single_block(text: &str, max_lines: usize) -> String {
    let max_chars = max_lines.max(1) * 120;
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let head_chars = max_chars * 3 / 5;
    let tail_chars = max_chars - head_chars;
    let head: String = text.chars().take(head_chars).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(
        "{head}\n\n[... {} chars truncated ...]\n\n{tail}",
        char_count - max_chars
    )
}

/// Truncate tool result content using outline or head-tail.
pub fn truncate_content(
    content: &[Content],
    tool_name: &str,
    tool_call_id: &str,
    tool_call_index: &HashMap<String, String>,
    max_lines: usize,
    prefer_outline: bool,
) -> Vec<Content> {
    let original_tokens = content_tokens(content);
    let head_tail = head_tail_content(content, max_lines);

    if prefer_outline {
        if let Some(outlined) = try_outline(content, tool_name, tool_call_id, tool_call_index) {
            if content_tokens(&outlined) < original_tokens
                && content_tokens(&outlined) <= content_tokens(&head_tail)
            {
                return outlined;
            }
        }
    }

    head_tail
}

fn head_tail_content(content: &[Content], max_lines: usize) -> Vec<Content> {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => Content::Text {
                text: truncate_text_head_tail(text, max_lines),
            },
            other => other.clone(),
        })
        .collect()
}
