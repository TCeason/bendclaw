//! Reclaim transform — always-on, lossless cleanup.
//!
//! 1. Clear expired `Retention::CurrentRun` tool results.
//! 2. Downgrade old base64 images to path references.

use crate::context::compaction::config::CompactionConfig;
use crate::context::tokens::content_tokens;
use crate::types::*;

/// Run reclaim on all messages. Returns transformed messages and stats updates.
pub fn run(
    messages: Vec<AgentMessage>,
    config: &CompactionConfig,
) -> (Vec<AgentMessage>, ReclaimStats) {
    let (messages, current_run_reclaimed) = clear_current_run(messages);
    let (messages, images_downgraded) = downgrade_images(messages, config.keep_recent_images);
    let stats = ReclaimStats {
        current_run_reclaimed,
        images_downgraded,
    };
    (messages, stats)
}

pub struct ReclaimStats {
    pub current_run_reclaimed: usize,
    pub images_downgraded: usize,
}

// ---------------------------------------------------------------------------
// CurrentRun clearing
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Image downgrade
// ---------------------------------------------------------------------------

fn downgrade_images(messages: Vec<AgentMessage>, keep_recent: usize) -> (Vec<AgentMessage>, usize) {
    // Find indices of messages with images, keep the most recent N.
    let image_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, msg)| has_downgradable_image(msg))
        .map(|(i, _)| i)
        .collect();

    let to_downgrade: std::collections::HashSet<usize> = if image_indices.len() > keep_recent {
        image_indices[..image_indices.len() - keep_recent]
            .iter()
            .copied()
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    if to_downgrade.is_empty() {
        return (messages, 0);
    }

    let mut downgraded = 0usize;
    let result = messages
        .into_iter()
        .enumerate()
        .map(|(idx, msg)| {
            if !to_downgrade.contains(&idx) {
                return msg;
            }
            match msg {
                AgentMessage::Llm(Message::User { content, timestamp }) => {
                    let mut new_content = content;
                    if downgrade_content(&mut new_content) {
                        downgraded += 1;
                    }
                    AgentMessage::Llm(Message::User {
                        content: new_content,
                        timestamp,
                    })
                }
                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content,
                    is_error,
                    timestamp,
                    retention,
                }) => {
                    let mut new_content = content;
                    if downgrade_content(&mut new_content) {
                        downgraded += 1;
                    }
                    AgentMessage::Llm(Message::ToolResult {
                        tool_call_id,
                        tool_name,
                        content: new_content,
                        is_error,
                        timestamp,
                        retention,
                    })
                }
                other => other,
            }
        })
        .collect();

    (result, downgraded)
}

fn has_downgradable_image(msg: &AgentMessage) -> bool {
    let content = match msg {
        AgentMessage::Llm(Message::User { content, .. }) => content,
        AgentMessage::Llm(Message::ToolResult { content, .. }) => content,
        _ => return false,
    };
    content.iter().any(|c| {
        matches!(c, Content::Image {
            source: ImageSource::Base64 { path: Some(_), .. },
            ..
        })
    })
}

fn downgrade_content(content: &mut [Content]) -> bool {
    let mut changed = false;
    for c in content.iter_mut() {
        if let Content::Image { source, .. } = c {
            if let ImageSource::Base64 {
                path: Some(path), ..
            } = source
            {
                let path_val = std::mem::take(path);
                *source = ImageSource::Path { path: path_val };
                changed = true;
            }
        }
    }
    changed
}
