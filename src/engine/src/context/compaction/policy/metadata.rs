//! Tool result → metadata summary formatting.
//!
//! Used by microcompact to replace full tool results with compact metadata
//! that preserves enough context for the model to decide if re-reading is needed.

use crate::types::*;

/// Generate a metadata summary for a tool result.
/// Falls back to `[cleared — {tool_name}]` if extraction fails.
pub fn to_metadata(
    tool_name: &str,
    content: &[Content],
    messages: &[AgentMessage],
    msg_idx: usize,
) -> Vec<Content> {
    let text = extract_text(content);
    let summary = match tool_name {
        "Read" => metadata_for_read(messages, msg_idx, &text),
        "Bash" => metadata_for_bash(messages, msg_idx, &text),
        "Edit" | "Write" => metadata_for_edit(messages, msg_idx),
        "WebFetch" => metadata_for_webfetch(messages, msg_idx, &text),
        _ => format!("[cleared — {tool_name}]"),
    };
    vec![Content::Text { text: summary }]
}

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

fn metadata_for_read(messages: &[AgentMessage], msg_idx: usize, text: &str) -> String {
    let path = find_tool_use_param(messages, msg_idx, "path").unwrap_or_else(|| "unknown".into());
    let lines = text.lines().count();
    format!("[cleared — {path}, {lines} lines]")
}

fn metadata_for_bash(messages: &[AgentMessage], msg_idx: usize, text: &str) -> String {
    let cmd = find_tool_use_param(messages, msg_idx, "command").unwrap_or_else(|| "unknown".into());
    let cmd_short = if cmd.len() > 80 {
        format!("{}...", &cmd[..77])
    } else {
        cmd
    };
    let lines = text.lines().count();
    // Try to extract exit code from text (common pattern: "Exit code: N")
    let exit_code = text
        .lines()
        .find_map(|l| {
            l.strip_prefix("Exit code: ")
                .and_then(|s| s.trim().parse::<i32>().ok())
        })
        .map(|c| format!(", exit {c}"))
        .unwrap_or_default();
    format!("[cleared — `{cmd_short}`{exit_code}, {lines} lines]")
}

fn metadata_for_edit(messages: &[AgentMessage], msg_idx: usize) -> String {
    let path = find_tool_use_param(messages, msg_idx, "path").unwrap_or_else(|| "unknown".into());
    format!("[cleared — {path}]")
}

fn metadata_for_webfetch(messages: &[AgentMessage], msg_idx: usize, text: &str) -> String {
    let url = find_tool_use_param(messages, msg_idx, "url").unwrap_or_else(|| "unknown".into());
    let chars = text.len();
    format!("[cleared — {url}, {chars} chars]")
}

/// Walk backwards from a ToolResult to find the matching ToolUse in the
/// preceding Assistant message, then extract a parameter value from its input.
fn find_tool_use_param(
    messages: &[AgentMessage],
    tool_result_idx: usize,
    param: &str,
) -> Option<String> {
    // Get the tool_call_id from the ToolResult
    let target_id = match &messages[tool_result_idx] {
        AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) => tool_call_id,
        _ => return None,
    };

    // Search backwards for the Assistant message containing this tool_call
    for msg in messages[..tool_result_idx].iter().rev() {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content {
                if let Content::ToolCall { id, arguments, .. } = block {
                    if id == target_id {
                        // Extract from serde_json::Value
                        return arguments
                            .get(param)
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                }
            }
        }
    }
    None
}
