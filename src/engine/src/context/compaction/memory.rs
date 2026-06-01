//! Memory extraction — builds compact memory from evicted messages.

use std::collections::HashSet;

use super::types::CompactionState;
use super::types::FileOps;
use crate::types::*;

/// Input for compact memory generation.
pub struct MemoryInput<'a> {
    /// Messages being evicted.
    pub evicted: &'a [AgentMessage],
    /// If a turn was split, the prefix portion (turn_start..cut_at).
    pub split_turn_prefix: Option<&'a [AgentMessage]>,
    /// Previous compaction state (for cumulative tracking).
    pub prev_state: Option<&'a CompactionState>,
}

/// Build a memory summary message that replaces the evicted zone.
pub fn build(input: &MemoryInput<'_>) -> AgentMessage {
    let mut sections: Vec<String> = Vec::new();

    // Section 1: Overview
    let msg_count = input.evicted.len();
    sections.push(format!("[Context compacted: {msg_count} messages removed]"));

    // Section 2: Completed user requests
    let requests = extract_user_requests(input.evicted);
    if !requests.is_empty() {
        let mut s = String::from("Completed requests (do not revisit):");
        for req in &requests {
            s.push_str("\n- ");
            s.push_str(req);
        }
        sections.push(s);
    }

    // Section 3: File operations (cumulative)
    let file_ops = extract_file_ops(input.evicted, input.prev_state);
    let file_section = format_file_ops(&file_ops);
    if !file_section.is_empty() {
        sections.push(file_section);
    }

    // Section 4: Key decisions from assistant
    let decisions = extract_decisions(input.evicted);
    if !decisions.is_empty() {
        let mut s = String::from("Key decisions:");
        for d in &decisions {
            s.push_str("\n- ");
            s.push_str(d);
        }
        sections.push(s);
    }

    // Section 5: Split turn context (provides extra structure for the split portion,
    // which is already included in `evicted` but benefits from explicit tool summary)
    if let Some(prefix) = input.split_turn_prefix {
        let summary = summarize_turn_prefix(prefix);
        if !summary.is_empty() {
            sections.push(format!("Current turn context (prefix removed):\n{summary}"));
        }
    }

    // Section 6: Environment discoveries (cumulative)
    let env = extract_env_discoveries(input.evicted, input.prev_state);
    if !env.is_empty() {
        let mut s = String::from("Environment:");
        for e in &env {
            s.push_str("\n- ");
            s.push_str(e);
        }
        sections.push(s);
    }

    // Section 7: Last assistant conclusion
    if let Some(conclusion) = latest_assistant_text(input.evicted) {
        sections.push(format!("Last assistant conclusion:\n{conclusion}"));
    }

    let text = sections.join("\n\n");
    AgentMessage::Llm(Message::User {
        content: vec![Content::Text { text }],
        timestamp: now_ms(),
    })
}

/// Build the cumulative state from evicted messages + previous state.
pub fn build_state(
    evicted: &[AgentMessage],
    split_prefix: Option<&[AgentMessage]>,
    prev_state: Option<&CompactionState>,
) -> CompactionState {
    let file_ops = extract_file_ops(evicted, prev_state);
    let env_discoveries = extract_env_discoveries(evicted, prev_state);
    let completed_requests = extract_user_requests(evicted);

    let mut all_requests = prev_state
        .map(|s| s.completed_requests.clone())
        .unwrap_or_default();
    all_requests.extend(completed_requests);
    // Keep bounded
    if all_requests.len() > 20 {
        all_requests = all_requests.split_off(all_requests.len() - 20);
    }

    // Include split prefix file ops
    let file_ops = if let Some(prefix) = split_prefix {
        let mut ops = file_ops;
        collect_file_ops_from_messages(prefix, &mut ops);
        ops
    } else {
        file_ops
    };

    CompactionState {
        file_ops,
        env_discoveries,
        completed_requests: all_requests,
        timestamp: now_ms(),
        generation: prev_state.map(|s| s.generation + 1).unwrap_or(1),
        last_summary: None,
    }
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

const ANCHOR_MAX_CHARS: usize = 200;
const CONCLUSION_MAX_CHARS: usize = 300;

pub(crate) fn extract_user_requests(messages: &[AgentMessage]) -> Vec<String> {
    let mut requests = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::User { content, .. }) = msg {
            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.starts_with("[Context compacted") {
                continue;
            }
            let short = if trimmed.len() > ANCHOR_MAX_CHARS {
                format!(
                    "{}…",
                    &trimmed[..trimmed.floor_char_boundary(ANCHOR_MAX_CHARS)]
                )
            } else {
                trimmed.to_string()
            };
            requests.push(short);
        }
    }
    requests
}

pub(crate) fn extract_file_ops(
    messages: &[AgentMessage],
    prev_state: Option<&CompactionState>,
) -> FileOps {
    let mut ops = prev_state.map(|s| s.file_ops.clone()).unwrap_or_default();
    collect_file_ops_from_messages(messages, &mut ops);
    ops
}

pub(crate) fn collect_file_ops_from_messages(messages: &[AgentMessage], ops: &mut FileOps) {
    for msg in messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content {
                if let Content::ToolCall {
                    name, arguments, ..
                } = block
                {
                    let path = arguments
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if let Some(path) = path {
                        match name.to_lowercase().as_str() {
                            "read" => {
                                ops.read.insert(path);
                            }
                            "write" => {
                                ops.written.insert(path);
                            }
                            "edit" => {
                                ops.edited.insert(path);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn format_file_ops(ops: &FileOps) -> String {
    let modified = ops.modified();
    let read_only = ops.read_only();

    if modified.is_empty() && read_only.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    if !modified.is_empty() {
        out.push_str("Files modified:");
        for f in &modified {
            out.push_str("\n- ");
            out.push_str(f);
        }
    }
    if !read_only.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("Files read:");
        for f in &read_only {
            out.push_str("\n- ");
            out.push_str(f);
        }
    }
    out
}

fn extract_decisions(messages: &[AgentMessage]) -> Vec<String> {
    let mut decisions = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content {
                if let Content::Text { text } = block {
                    // Look for short decisive statements
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if trimmed.len() > 20
                            && trimmed.len() <= 200
                            && (trimmed.contains("because")
                                || trimmed.contains("instead of")
                                || trimmed.contains("I'll use")
                                || trimmed.contains("chose")
                                || trimmed.contains("decision"))
                        {
                            decisions.push(trimmed.to_string());
                            if decisions.len() >= 5 {
                                return decisions;
                            }
                        }
                    }
                }
            }
        }
    }
    decisions
}

pub(crate) fn extract_env_discoveries(
    messages: &[AgentMessage],
    prev_state: Option<&CompactionState>,
) -> Vec<String> {
    let mut env = prev_state
        .map(|s| s.env_discoveries.clone())
        .unwrap_or_default();

    for (i, msg) in messages.iter().enumerate() {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name,
            is_error: false,
            content,
            tool_call_id,
            ..
        }) = msg
        {
            if tool_name != "bash" && tool_name != "Bash" {
                continue;
            }

            let cmd =
                find_tool_call_param(messages, i, tool_call_id, "command").unwrap_or_default();

            let is_env_probe = cmd.contains("which ")
                || cmd.contains("find /")
                || cmd.contains("rustup")
                || cmd.starts_with("export PATH=");

            if !is_env_probe {
                continue;
            }

            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            for line in text.lines().take(5) {
                let trimmed = line.trim();
                if trimmed.contains("/bin/")
                    || trimmed.contains("/usr/")
                    || trimmed.contains("toolchain")
                    || trimmed.contains("rustup")
                {
                    let entry = if trimmed.len() > 100 {
                        format!("{}...", &trimmed[..trimmed.floor_char_boundary(97)])
                    } else {
                        trimmed.to_string()
                    };
                    if !env.contains(&entry) {
                        env.push(entry);
                    }
                    if env.len() >= 10 {
                        return env;
                    }
                    break;
                }
            }
        }
    }
    env
}

fn summarize_turn_prefix(prefix: &[AgentMessage]) -> String {
    let mut parts = Vec::new();

    // Extract user request
    for msg in prefix {
        if let AgentMessage::Llm(Message::User { content, .. }) = msg {
            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let short = if trimmed.len() > ANCHOR_MAX_CHARS {
                    format!(
                        "{}…",
                        &trimmed[..trimmed.floor_char_boundary(ANCHOR_MAX_CHARS)]
                    )
                } else {
                    trimmed.to_string()
                };
                parts.push(format!("User asked: {short}"));
            }
        }
    }

    // Extract tool calls summary
    let mut tool_calls: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for msg in prefix {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content {
                if let Content::ToolCall {
                    name, arguments, ..
                } = block
                {
                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let key = format!("{name}({path})");
                    if seen.insert(key.clone()) {
                        tool_calls.push(key);
                    }
                }
            }
        }
    }
    if !tool_calls.is_empty() {
        let summary = if tool_calls.len() <= 5 {
            tool_calls.join(", ")
        } else {
            format!(
                "{}, ... ({} more)",
                tool_calls[..5].join(", "),
                tool_calls.len() - 5
            )
        };
        parts.push(format!("Tools used: {summary}"));
    }

    parts.join("\n")
}

pub(crate) fn latest_assistant_text(messages: &[AgentMessage]) -> Option<String> {
    for msg in messages.iter().rev() {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for c in content.iter().rev() {
                if let Content::Text { text } = c {
                    let t = text.trim();
                    if t.is_empty() || is_filler(t) || t.starts_with("[Summary]") {
                        continue;
                    }
                    let trimmed = if t.chars().count() > CONCLUSION_MAX_CHARS {
                        format!(
                            "{}…",
                            t.chars().take(CONCLUSION_MAX_CHARS).collect::<String>()
                        )
                    } else {
                        t.to_string()
                    };
                    return Some(trimmed);
                }
            }
        }
    }
    None
}

fn is_filler(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    matches!(
        t.as_str(),
        "done"
            | "done."
            | "ok"
            | "ok."
            | "sure"
            | "sure."
            | "i'll fix this"
            | "let me check"
            | "let me look"
    )
}

fn find_tool_call_param(
    messages: &[AgentMessage],
    tool_result_idx: usize,
    target_id: &str,
    param: &str,
) -> Option<String> {
    for msg in messages[..tool_result_idx].iter().rev() {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content {
                if let Content::ToolCall { id, arguments, .. } = block {
                    if id == target_id {
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

fn now_ms() -> u64 {
    crate::context::now_ms()
}
