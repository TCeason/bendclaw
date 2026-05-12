//! Downgrade old images with a known disk path from `Base64` to `Path`.
//!
//! **L0 — always-on**: runs unconditionally regardless of budget.
//!
//! Image base64 blobs dominate context size (5–10k tokens each, sometimes
//! much more at the provider level) and are not compressible. Before any
//! other compaction phase looks at the message list, we reclaim every old
//! image whose origin is still on disk by swapping
//! `ImageSource::Base64 { data, path: Some(p) }` for
//! `ImageSource::Path { path: p }`. Provider-side behavior is unchanged —
//! the engine re-reads the bytes from disk when actually calling the model.
//!
//! The most recent image in the message list is left untouched so the model
//! can still reason about the thing it was just shown without paying an
//! extra disk read on the very next turn.

use crate::context::compaction::phase::PhaseContext;
use crate::context::compaction::phase::PhaseResult;
use crate::context::compaction::CompactionAction;
use crate::context::compaction::CompactionMethod;
use crate::context::tokens::content_tokens;
use crate::types::*;

/// Locate the index of the most recent image that still has a usable path
/// backing (either already `Path`, or `Base64` with `path: Some(_)`).
/// That one is kept in its current form — other images become candidates
/// for downgrade.
fn find_keep_index(messages: &[AgentMessage]) -> Option<usize> {
    for (idx, msg) in messages.iter().enumerate().rev() {
        let content = match msg {
            AgentMessage::Llm(Message::User { content, .. }) => content,
            AgentMessage::Llm(Message::Assistant { content, .. }) => content,
            AgentMessage::Llm(Message::ToolResult { content, .. }) => content,
            _ => continue,
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
            return Some(idx);
        }
    }
    None
}

fn downgrade_content(content: &mut [Content]) -> bool {
    let mut changed = false;
    for c in content.iter_mut() {
        if let Content::Image { source, .. } = c {
            if let ImageSource::Base64 {
                path: Some(path), ..
            } = source
            {
                let path = std::mem::take(path);
                *source = ImageSource::Path { path };
                changed = true;
            }
        }
    }
    changed
}

pub fn run(messages: Vec<AgentMessage>, _ctx: &PhaseContext) -> PhaseResult {
    let keep_idx = find_keep_index(&messages);
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
                AgentMessage::Llm(Message::Assistant {
                    content,
                    stop_reason,
                    model,
                    provider,
                    usage,
                    timestamp,
                    error_message,
                    response_id,
                }) => {
                    let before_tokens = content_tokens(&content);
                    let mut new_content = content;
                    if downgrade_content(&mut new_content) {
                        let after_tokens = content_tokens(&new_content);
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: "assistant".into(),
                            method: CompactionMethod::ImageStripped,
                            before_tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });
                    }
                    AgentMessage::Llm(Message::Assistant {
                        content: new_content,
                        stop_reason,
                        model,
                        provider,
                        usage,
                        timestamp,
                        error_message,
                        response_id,
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

    PhaseResult {
        messages: result,
        actions,
    }
}
