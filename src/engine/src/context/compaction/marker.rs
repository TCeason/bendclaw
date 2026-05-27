use crate::types::*;

const ANCHOR_MAX_CHARS: usize = 200;

pub fn build_full_marker(pre_drop_messages: &[AgentMessage], removed: usize) -> AgentMessage {
    full_marker(pre_drop_messages, &format!("{removed} messages removed"))
}

pub(crate) fn build_fallback_marker() -> AgentMessage {
    minimal("messages removed")
}

fn minimal(note: &str) -> AgentMessage {
    text_message(&format!("[Context compacted: {note}]"))
}

fn full_marker(pre_drop_messages: &[AgentMessage], count_note: &str) -> AgentMessage {
    let mut out = format!("[Context compacted: {count_note}]");

    let completed = retained_user_texts(pre_drop_messages);
    if !completed.is_empty() {
        out.push_str("\n\nRetained early context contains these COMPLETED tasks (already handled, do not revisit):\n");
        for text in completed {
            out.push_str("- ");
            out.push_str(&text);
            out.push('\n');
        }
    }

    let modifications = extract_file_modifications(pre_drop_messages);
    if !modifications.is_empty() {
        out.push_str("\nFiles already modified (do not re-apply these edits):\n");
        for m in &modifications {
            out.push_str("- ");
            out.push_str(m);
            out.push('\n');
        }
    }

    let env = extract_env_discoveries(pre_drop_messages);
    if !env.is_empty() {
        out.push_str("\nEnvironment (discovered earlier):\n");
        for e in &env {
            out.push_str("- ");
            out.push_str(e);
            out.push('\n');
        }
    }

    if let Some(conclusion) = latest_assistant_text(pre_drop_messages) {
        out.push_str("\nLast assistant conclusion:\n");
        out.push_str(&conclusion);
        out.push('\n');
    }

    if let Some(text) = latest_user_text(pre_drop_messages) {
        out.push_str("\nMost recent user request (verbatim):\n");
        out.push_str(&text);
    }

    text_message(&out)
}

fn text_message(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        timestamp: now_ms(),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn retained_user_texts(messages: &[AgentMessage]) -> Vec<String> {
    let latest = latest_user_text(messages);
    let mut out = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::User { content, .. }) = msg {
            for c in content {
                if let Content::Text { text } = c {
                    let t = text.trim();
                    if marker_candidate(t) && Some(t) != latest.as_deref() {
                        out.push(trim_text(t));
                    }
                }
            }
        }
    }
    out
}

fn latest_user_text(messages: &[AgentMessage]) -> Option<String> {
    for msg in messages.iter().rev() {
        if let AgentMessage::Llm(Message::User { content, .. }) = msg {
            for c in content {
                if let Content::Text { text } = c {
                    let t = text.trim();
                    if marker_candidate(t) {
                        return Some(trim_text(t));
                    }
                }
            }
        }
    }
    None
}

fn marker_candidate(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with("<system-reminder>")
        && !text.starts_with("[Context compacted")
}

fn trim_text(text: &str) -> String {
    if text.chars().count() > ANCHOR_MAX_CHARS {
        format!(
            "{}…",
            text.chars().take(ANCHOR_MAX_CHARS).collect::<String>()
        )
    } else {
        text.to_string()
    }
}

const CONCLUSION_MAX_CHARS: usize = 300;

fn extract_file_modifications(messages: &[AgentMessage]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name,
            is_error: false,
            tool_call_id,
            ..
        }) = msg
        {
            let action = match tool_name.as_str() {
                "edit" => "edited",
                "write" => "created",
                _ => continue,
            };

            if let Some(path) = find_tool_call_param(messages, i, tool_call_id, "file_path")
                .or_else(|| find_tool_call_param(messages, i, tool_call_id, "path"))
            {
                if seen.insert(path.clone()) {
                    result.push(format!("{path} ({action})"));
                    if result.len() >= 10 {
                        break;
                    }
                }
            }
        }
    }
    result
}

fn extract_env_discoveries(messages: &[AgentMessage]) -> Vec<String> {
    let mut result = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name,
            is_error: false,
            content,
            tool_call_id,
            ..
        }) = msg
        {
            if tool_name != "bash" {
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
                        format!("{}...", &trimmed[..97])
                    } else {
                        trimmed.to_string()
                    };
                    result.push(entry);
                    if result.len() >= 5 {
                        return result;
                    }
                    break;
                }
            }
        }
    }
    result
}

fn latest_assistant_text(messages: &[AgentMessage]) -> Option<String> {
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
