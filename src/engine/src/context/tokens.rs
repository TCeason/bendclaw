//! Local, model-agnostic token *approximation*.
//!
//! This is deliberately NOT a real tokenizer. The accurate, model-specific
//! context size always comes from the provider's own `usage` (see
//! `ContextTracker::estimate_context_tokens`, which anchors on the latest
//! assistant usage embedded in the transcript). A real tokenizer here would
//! only be right for one model family and wrong for every other, so we mirror
//! pi's cheap `Math.ceil(string.length / 4)` heuristic. JavaScript
//! `string.length` counts UTF-16 code units, so this module does the same.
//!
//! It is used only where an exact count is unnecessary:
//!   - sizing the small trailing delta since the last provider response,
//!   - relative sizing inside compaction (which messages to cut, before/after),
//!   - per-role breakdowns shown in observability events (labelled estimates).

use crate::provider::ToolDefinition;
use crate::types::*;

/// pi models an image as 4,800 characters before applying chars / 4.
const IMAGE_FIXED_CHAR_ESTIMATE: usize = 4_800;

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn chars_to_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

/// Approximate tokens for a text string using pi's UTF-16 chars / 4 heuristic.
pub fn estimate_tokens(text: &str) -> usize {
    chars_to_tokens(utf16_len(text))
}

fn content_chars(content: &[Content]) -> usize {
    content
        .iter()
        .map(|block| match block {
            Content::Text { text } => utf16_len(text),
            Content::Image { .. } => IMAGE_FIXED_CHAR_ESTIMATE,
            Content::Thinking { thinking, .. } => utf16_len(thinking),
            Content::ToolCall {
                name, arguments, ..
            } => utf16_len(name) + utf16_len(&arguments.to_string()),
        })
        .sum()
}

/// Estimate tokens for one message. As in pi, all visible content is summed
/// before rounding; role envelopes and tool names add no synthetic overhead.
pub fn message_tokens(msg: &AgentMessage) -> usize {
    match msg {
        AgentMessage::Llm(message) => match message {
            Message::User { content, .. }
            | Message::Assistant { content, .. }
            | Message::ToolResult { content, .. } => chars_to_tokens(content_chars(content)),
        },
        // Extension messages are UI-only and never enter the LLM context.
        AgentMessage::Extension(_) => 0,
    }
}

pub fn content_tokens(content: &[Content]) -> usize {
    chars_to_tokens(content_chars(content))
}

/// Estimate total tokens for a message list
pub fn total_tokens(messages: &[AgentMessage]) -> usize {
    messages.iter().map(message_tokens).sum()
}

/// Estimate tokens for a single `Content` block.
fn single_content_tokens(content: &Content) -> usize {
    let chars = match content {
        Content::Text { text } => utf16_len(text),
        Content::Image { .. } => IMAGE_FIXED_CHAR_ESTIMATE,
        Content::Thinking { thinking, .. } => utf16_len(thinking),
        Content::ToolCall {
            name, arguments, ..
        } => utf16_len(name) + utf16_len(&arguments.to_string()),
    };
    chars_to_tokens(chars)
}

/// Estimate tokens for tool definitions.
pub fn tool_definition_tokens(tools: &[ToolDefinition]) -> usize {
    tools
        .iter()
        .map(|tool| match serde_json::to_string(tool) {
            Ok(json) => estimate_tokens(&json),
            Err(_) => estimate_tokens(&tool.name) + estimate_tokens(&tool.description),
        })
        .sum()
}

/// Compute pre-aggregated stats from LLM messages.
///
/// Image tokens are counted as a separate dimension (not included in
/// user/assistant/tool_result tokens), so:
///   total = user_tokens + assistant_tokens + tool_result_tokens + image_tokens
pub fn compute_call_stats(messages: &[Message]) -> LlmCallStats {
    compute_call_stats_iter(messages.iter())
}

/// Compute stats from `AgentMessage` slice (filters to LLM messages only).
pub fn compute_call_stats_from_agent_messages(messages: &[AgentMessage]) -> LlmCallStats {
    compute_call_stats_iter(messages.iter().filter_map(|m| m.as_llm()))
}

fn add_image_stats(stats: &mut LlmCallStats, content: &Content, tokens: usize) {
    if let Content::Image { source, .. } = content {
        stats.image_count += 1;
        stats.image_tokens += tokens;
        match source {
            ImageSource::Path { .. } => stats.image_path_count += 1,
            ImageSource::Base64 { .. } => stats.image_base64_count += 1,
        }
    }
}

fn compute_call_stats_iter<'a>(messages: impl Iterator<Item = &'a Message>) -> LlmCallStats {
    let mut stats = LlmCallStats::default();

    for msg in messages {
        match msg {
            Message::User { content, .. } => {
                stats.user_count += 1;
                for c in content {
                    let tok = single_content_tokens(c);
                    if matches!(c, Content::Image { .. }) {
                        add_image_stats(&mut stats, c, tok);
                    } else {
                        stats.user_tokens += tok;
                    }
                }
            }
            Message::Assistant { content, .. } => {
                stats.assistant_count += 1;
                for c in content {
                    let tok = single_content_tokens(c);
                    if matches!(c, Content::Image { .. }) {
                        add_image_stats(&mut stats, c, tok);
                    } else {
                        stats.assistant_tokens += tok;
                    }
                }
            }
            Message::ToolResult {
                content, tool_name, ..
            } => {
                stats.tool_result_count += 1;
                let mut msg_tokens = 0usize;
                for c in content {
                    let tok = single_content_tokens(c);
                    if matches!(c, Content::Image { .. }) {
                        add_image_stats(&mut stats, c, tok);
                    } else {
                        stats.tool_result_tokens += tok;
                        msg_tokens += tok;
                    }
                }
                stats.tool_details.push((tool_name.clone(), msg_tokens));
            }
        }
    }

    stats.tool_details.sort_by(|a, b| b.1.cmp(&a.1));
    stats
}
