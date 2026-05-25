//! Microcompact pass — turn-based, quantity-driven tool result clearing.
//!
//! Triggered when compactable (non-cleared) tool results exceed a threshold.
//! Three tiers: keep_full (newest) → keep_meta (middle) → clear (oldest).
//! Does NOT use LLM — purely structural.

use crate::context::compaction::pass::Pass;
use crate::context::compaction::pass::PassContext;
use crate::context::compaction::pass::PassLevel;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::policy::metadata::to_metadata;
use crate::context::compaction::policy::tool_policy::is_compactable_tool_result;
use crate::context::compaction::types::CompactionAction;
use crate::context::compaction::types::CompactionMethod;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub struct Microcompact;

impl Pass for Microcompact {
    fn level(&self) -> PassLevel {
        PassLevel::Microcompact
    }

    fn should_run(&self, ctx: &PassContext<'_>) -> bool {
        // Purely count-driven — fire whenever enough compactable tool results
        // accumulate, regardless of token pressure. This matches Claude Code's
        // behavior where microcompact is independent of the autocompact threshold.
        ctx.pressure.compactable_tool_result_count >= ctx.config.microcompact_trigger
    }

    fn run(&self, messages: Vec<AgentMessage>, ctx: &PassContext<'_>) -> PassResult {
        let config = ctx.config;

        // Find turn boundaries (assistant messages) from the end
        let recent_boundary = find_turn_boundary(&messages, 2);

        // Collect compactable tool result indices (only before recent boundary)
        let compactable: Vec<usize> = messages
            .iter()
            .enumerate()
            .take(recent_boundary)
            .filter_map(|(i, msg)| {
                if is_compactable_and_not_cleared(msg) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        if compactable.len() < config.microcompact_trigger {
            return PassResult {
                messages,
                actions: vec![],
            };
        }

        let total = compactable.len();
        let keep_full = config.microcompact_keep_full.min(total);
        let keep_meta = config
            .microcompact_keep_meta
            .min(total.saturating_sub(keep_full));

        let mut result = messages;
        let mut actions = Vec::new();

        // Process from newest to oldest: assign tiers
        // Microcompact is purely count-driven — always apply tier-based clearing
        // regardless of token budget. This prevents unbounded context growth in
        // sessions that stay below the autocompact threshold.
        for (rank, &idx) in compactable.iter().rev().enumerate() {
            if rank < keep_full {
                // Tier 0: keep full content
                continue;
            }

            if let AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
                retention,
            }) = &result[idx]
            {
                let before_tokens = content_tokens(content);

                let replacement = if rank < keep_full + keep_meta {
                    // Tier 1: replace with metadata
                    to_metadata(tool_name, content, &result, idx)
                } else {
                    // Tier 2: fully clear
                    vec![Content::Text {
                        text: "[cleared]".into(),
                    }]
                };

                let after_tokens = content_tokens(&replacement);
                if after_tokens >= before_tokens {
                    continue;
                }

                actions.push(CompactionAction {
                    index: idx,
                    tool_name: tool_name.clone(),
                    method: CompactionMethod::AgeCleared,
                    before_tokens,
                    after_tokens,
                    end_index: None,
                    related_count: None,
                });

                result[idx] = AgentMessage::Llm(Message::ToolResult {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content: replacement,
                    is_error: *is_error,
                    timestamp: *timestamp,
                    retention: *retention,
                });
            }
        }

        // Truncate large tool_use inputs for cleared results
        truncate_cleared_tool_use_inputs(&mut result, &compactable, keep_full);

        // --- Image age clearing ---
        // Strip old images beyond recent boundary, keeping the most recent ones.
        let keep_images = config.microcompact_keep_images;
        let image_indices: Vec<usize> = result
            .iter()
            .enumerate()
            .take(recent_boundary)
            .filter_map(|(i, msg)| {
                if let AgentMessage::Llm(Message::User { content, .. }) = msg {
                    if content.iter().any(|c| matches!(c, Content::Image { .. })) {
                        return Some(i);
                    }
                }
                None
            })
            .collect();

        for (rank, &idx) in image_indices.iter().rev().enumerate() {
            if rank < keep_images {
                continue;
            }
            if let AgentMessage::Llm(Message::User {
                content, timestamp, ..
            }) = &result[idx]
            {
                let before_tokens = content_tokens(content);
                let stripped: Vec<Content> = content
                    .iter()
                    .map(|c| match c {
                        Content::Image { source, .. } => {
                            let path_hint = match source {
                                ImageSource::Path { path } => path.clone(),
                                ImageSource::Base64 { path, .. } => {
                                    path.clone().unwrap_or_default()
                                }
                            };
                            if path_hint.is_empty() {
                                Content::Text {
                                    text: "[image]".into(),
                                }
                            } else {
                                Content::Text {
                                    text: format!("[image: {path_hint}]"),
                                }
                            }
                        }
                        other => other.clone(),
                    })
                    .collect();
                let after_tokens = content_tokens(&stripped);
                if after_tokens >= before_tokens {
                    continue;
                }
                actions.push(CompactionAction {
                    index: idx,
                    tool_name: "user".into(),
                    method: CompactionMethod::ImageStripped,
                    before_tokens,
                    after_tokens,
                    end_index: None,
                    related_count: None,
                });
                result[idx] = AgentMessage::Llm(Message::User {
                    content: stripped,
                    timestamp: *timestamp,
                });
            }
        }

        PassResult {
            messages: result,
            actions,
        }
    }
}

/// Find the message index that marks the boundary of N recent turns.
/// A "turn" starts at an Assistant message.
fn find_turn_boundary(messages: &[AgentMessage], keep_turns: usize) -> usize {
    let mut turns_seen = 0;
    for (i, msg) in messages.iter().enumerate().rev() {
        if matches!(msg, AgentMessage::Llm(Message::Assistant { .. })) {
            turns_seen += 1;
            if turns_seen >= keep_turns {
                return i;
            }
        }
    }
    messages.len() // not enough turns — protect everything
}

fn is_compactable_and_not_cleared(msg: &AgentMessage) -> bool {
    if let AgentMessage::Llm(Message::ToolResult {
        tool_name, content, ..
    }) = msg
    {
        let text_len: usize = content
            .iter()
            .map(|c| match c {
                Content::Text { text } => text.len(),
                _ => 0,
            })
            .sum();
        is_compactable_tool_result(tool_name, text_len)
    } else {
        false
    }
}

/// For tool results that were cleared, truncate the corresponding tool_use input
/// if it's large (e.g. Write/Edit with full file content).
fn truncate_cleared_tool_use_inputs(
    messages: &mut [AgentMessage],
    compactable_indices: &[usize],
    keep_full: usize,
) {
    const MAX_TOOL_USE_INPUT_CHARS: usize = 200;

    // Collect tool_call_ids that were cleared (not in keep_full tier)
    let cleared_ids: std::collections::HashSet<String> = compactable_indices
        .iter()
        .rev()
        .skip(keep_full)
        .filter_map(|&idx| {
            if let AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) = &messages[idx] {
                Some(tool_call_id.clone())
            } else {
                None
            }
        })
        .collect();

    if cleared_ids.is_empty() {
        return;
    }

    // Find and truncate matching tool_call blocks in assistant messages
    for msg in messages.iter_mut() {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for block in content.iter_mut() {
                if let Content::ToolCall { id, arguments, .. } = block {
                    if cleared_ids.contains(id.as_str()) {
                        let truncated = truncate_tool_use_input(
                            arguments.to_string(),
                            MAX_TOOL_USE_INPUT_CHARS,
                        );
                        if let Some(truncated) = truncated {
                            *arguments = serde_json::json!({
                                "_truncated": truncated
                            });
                        }
                    }
                }
            }
        }
    }
}

fn truncate_tool_use_input(input: String, max_chars: usize) -> Option<String> {
    if input.chars().count() <= max_chars {
        return None;
    }

    let truncated = input.chars().take(max_chars).collect::<String>();
    Some(format!("{truncated}...[truncated]"))
}

#[cfg(test)]
mod tests {
    use super::truncate_tool_use_input;

    #[test]
    fn truncate_tool_use_input_is_utf8_safe() {
        let input = "界".repeat(201);
        let truncated = truncate_tool_use_input(input, 200).expect("expected truncation");

        assert_eq!(truncated.chars().take(200).count(), 200);
        assert!(truncated.ends_with("...[truncated]"));
    }
}
