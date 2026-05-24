//! Reclaim pass — always-on, lossless cleanup.
//!
//! Two sub-transforms:
//! 1. Clear expired `Retention::CurrentRun` tool results.
//! 2. Downgrade old base64 images to path references.

use crate::context::compaction::pass::Pass;
use crate::context::compaction::pass::PassContext;
use crate::context::compaction::pass::PassLevel;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::types::CompactionAction;
use crate::context::compaction::types::CompactionMethod;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub struct Reclaim;

impl Pass for Reclaim {
    fn level(&self) -> PassLevel {
        PassLevel::Reclaim
    }

    fn should_run(&self, _ctx: &PassContext<'_>) -> bool {
        true // always-on
    }

    fn run(&self, messages: Vec<AgentMessage>, _ctx: &PassContext<'_>) -> PassResult {
        let (messages, mut actions) = clear_current_run(messages);
        let (messages, image_actions) = downgrade_images(messages);
        actions.extend(image_actions);
        PassResult { messages, actions }
    }
}

// -- CurrentRun clearing --

fn clear_current_run(messages: Vec<AgentMessage>) -> (Vec<AgentMessage>, Vec<CompactionAction>) {
    let mut has_user_after = vec![false; messages.len()];
    let mut seen_user = false;
    for i in (0..messages.len()).rev() {
        has_user_after[i] = seen_user;
        if matches!(&messages[i], AgentMessage::Llm(Message::User { .. })) {
            seen_user = true;
        }
    }

    let mut actions = Vec::new();
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
                let after_tokens = content_tokens(&replacement);

                actions.push(CompactionAction {
                    index: idx,
                    tool_name: tool_name.clone(),
                    method: CompactionMethod::LifecycleReclaimed,
                    before_tokens,
                    after_tokens,
                    end_index: None,
                    related_count: None,
                });

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

    (result, actions)
}
// -- Image path downgrade --

fn downgrade_images(messages: Vec<AgentMessage>) -> (Vec<AgentMessage>, Vec<CompactionAction>) {
    // Keep the most recent image-bearing message intact
    let keep_idx = messages.iter().enumerate().rev().find_map(|(idx, msg)| {
        let content = match msg {
            AgentMessage::Llm(Message::User { content, .. }) => content,
            AgentMessage::Llm(Message::Assistant { content, .. }) => content,
            AgentMessage::Llm(Message::ToolResult { content, .. }) => content,
            _ => return None,
        };
        let has_image = content.iter().any(|c| {
            matches!(
                c,
                Content::Image {
                    source: ImageSource::Path { .. },
                    ..
                } | Content::Image {
                    source: ImageSource::Base64 { path: Some(_), .. },
                    ..
                }
            )
        });
        if has_image {
            Some(idx)
        } else {
            None
        }
    });

    let mut actions = Vec::new();
    let result = messages
        .into_iter()
        .enumerate()
        .map(|(idx, msg)| {
            if Some(idx) == keep_idx {
                return msg;
            }
            match msg {
                AgentMessage::Llm(Message::User { content, timestamp }) => {
                    let before_tokens = content_tokens(&content);
                    let mut new_content = content;
                    if downgrade_content(&mut new_content) {
                        let after_tokens = content_tokens(&new_content);
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: "user".into(),
                            method: CompactionMethod::ImageStripped,
                            before_tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });
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
                    let before_tokens = content_tokens(&content);
                    let mut new_content = content;
                    if downgrade_content(&mut new_content) {
                        let after_tokens = content_tokens(&new_content);
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: tool_name.clone(),
                            method: CompactionMethod::ImageStripped,
                            before_tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });
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

    (result, actions)
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
