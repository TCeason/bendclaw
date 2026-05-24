use crate::types::*;

const ANCHOR_MAX_CHARS: usize = 200;

pub(crate) fn build_full_marker(
    pre_drop_messages: &[AgentMessage],
    removed: usize,
) -> AgentMessage {
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
