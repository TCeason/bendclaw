//! Input filtering: run filters on user messages before the LLM call.

use tokio::sync::mpsc;

use crate::types::*;

/// Apply input filters to prompts. Returns filtered prompts, or `None` if rejected.
pub(super) fn apply_input_filters(
    prompts: Vec<AgentMessage>,
    filters: &[std::sync::Arc<dyn InputFilter>],
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Option<Vec<AgentMessage>> {
    if filters.is_empty() {
        return Some(prompts);
    }

    let user_text: String = prompts
        .iter()
        .filter_map(|m| {
            if let AgentMessage::Llm(Message::User { content, .. }) = m {
                Some(
                    content
                        .iter()
                        .filter_map(|c| {
                            if let Content::Text { text } = c {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut warnings: Vec<String> = Vec::new();
    for filter in filters {
        match filter.filter(&user_text) {
            FilterResult::Pass => {}
            FilterResult::Warn(w) => warnings.push(w),
            FilterResult::Reject(reason) => {
                tx.send(AgentEvent::Error {
                    error: AgentErrorInfo {
                        kind: AgentErrorKind::InputRejected,
                        message: reason,
                    },
                })
                .ok();
                tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
                return None;
            }
        }
    }

    // Append warnings to the last user message's content
    if !warnings.is_empty() {
        let warning_text = warnings
            .iter()
            .map(|w| format!("[Warning: {}]", w))
            .collect::<Vec<_>>()
            .join("\n");

        let mut modified = prompts;
        for msg in modified.iter_mut().rev() {
            if let AgentMessage::Llm(Message::User { content, .. }) = msg {
                content.push(Content::Text { text: warning_text });
                break;
            }
        }
        Some(modified)
    } else {
        Some(prompts)
    }
}
