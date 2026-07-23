//! Serialize messages to text for LLM summarization.

use super::types::SummarizerInput;
use crate::context::compaction::memory;
use crate::context::compaction::types::CompactionState;
use crate::types::*;

/// Max chars per tool result in serialized output (prevents bloat).
const TOOL_RESULT_MAX_CHARS: usize = 2000;

/// Prepare a SummarizerInput from raw messages and state.
/// Bridge between executor (which has messages) and summarizer (which wants text).
pub fn prepare_input(
    evicted: &[AgentMessage],
    split_prefix: Option<&[AgentMessage]>,
    prev_state: Option<&CompactionState>,
) -> SummarizerInput {
    let conversation = serialize_messages(evicted);
    let turn_prefix = split_prefix.map(serialize_messages);
    let previous_summary = prev_state.and_then(|s| s.last_summary.clone());
    let file_ops = memory::extract_file_ops(evicted, prev_state);
    let completed_requests = memory::extract_user_requests(evicted);
    let env_discoveries = memory::extract_env_discoveries(evicted, prev_state);
    let last_conclusion = memory::latest_assistant_text(evicted);

    SummarizerInput {
        conversation,
        turn_prefix,
        previous_summary,
        custom_instructions: None,
        file_ops,
        evicted_count: evicted.len(),
        completed_requests,
        env_discoveries,
        last_conclusion,
    }
}

/// Serialize messages to a text representation for the LLM.
pub fn serialize_messages(messages: &[AgentMessage]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        match msg {
            AgentMessage::Llm(Message::User { content, .. }) => {
                let text = extract_text_content(content);
                if !text.is_empty() {
                    parts.push(format!("[User]: {text}"));
                }
            }
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                let mut text_parts: Vec<&str> = Vec::new();
                let mut thinking_parts: Vec<&str> = Vec::new();
                let mut tool_calls: Vec<String> = Vec::new();

                for block in content {
                    match block {
                        Content::Text { text } => text_parts.push(text),
                        Content::Thinking { thinking, .. } => thinking_parts.push(thinking),
                        Content::ToolCall {
                            name, arguments, ..
                        } => {
                            let args_str = arguments.to_string();
                            tool_calls.push(format!("{name}({args_str})"));
                        }
                        _ => {}
                    }
                }

                if !thinking_parts.is_empty() {
                    parts.push(format!(
                        "[Assistant thinking]: {}",
                        thinking_parts.join("\n")
                    ));
                }
                if !text_parts.is_empty() {
                    parts.push(format!("[Assistant]: {}", text_parts.join("\n")));
                }
                if !tool_calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", tool_calls.join("; ")));
                }
            }
            AgentMessage::Llm(Message::ToolResult { content, .. }) => {
                let text = extract_text_content(content);
                if !text.is_empty() {
                    let truncated = truncate_for_summary(&text, TOOL_RESULT_MAX_CHARS);
                    parts.push(format!("[Tool result]: {truncated}"));
                }
            }
            _ => {}
        }
    }

    parts.join("\n\n")
}

fn extract_text_content(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    // `max_chars` is a byte budget; snap to a char boundary so multi-byte
    // characters (e.g. CJK, Devanagari) are never split mid-codepoint.
    let end = text.floor_char_boundary(max_chars);
    let truncated_chars = text.len() - end;
    format!(
        "{}\n\n[... {} more characters truncated]",
        &text[..end],
        truncated_chars
    )
}
