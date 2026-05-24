//! Shrink pass — budget-gated truncation of oversized tool results and user messages.

use super::strategy::*;
use super::truncate::*;
use crate::context::compaction::pass::Pass;
use crate::context::compaction::pass::PassContext;
use crate::context::compaction::pass::PassLevel;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::policy::tool_policy;
use crate::context::compaction::types::CompactionAction;
use crate::context::compaction::types::CompactionMethod;
use crate::context::tokens::content_tokens;
use crate::types::*;

pub struct Shrink;

impl Pass for Shrink {
    fn level(&self) -> PassLevel {
        PassLevel::Shrink
    }

    fn should_run(&self, ctx: &PassContext<'_>) -> bool {
        let threshold = oversize_threshold(ctx);
        ctx.pressure.message_tokens > ctx.config.compact_trigger()
            || ctx.pressure.estimate_pressure
            || ctx.pressure.max_tool_result_tokens >= threshold
            || ctx.pressure.max_user_tokens >= threshold
    }

    fn run(&self, messages: Vec<AgentMessage>, ctx: &PassContext<'_>) -> PassResult {
        let tool_call_index = build_tool_call_index(&messages);
        let len = messages.len();
        let recent_boundary = len.saturating_sub(ctx.config.keep_recent);
        let oversize_token_threshold = oversize_threshold(ctx);

        let mut running_tokens = ctx.pressure.message_tokens;
        let mut actions = Vec::new();
        let mut result = Vec::with_capacity(len);

        for (idx, msg) in messages.into_iter().enumerate() {
            let is_recent = idx >= recent_boundary;
            let is_pinned = idx < ctx.config.keep_first;

            // --- User messages ---
            if let AgentMessage::Llm(Message::User { content, timestamp }) = &msg {
                let action = classify_user_action(
                    is_pinned,
                    is_recent,
                    running_tokens,
                    ctx.config.compact_target(),
                    ctx.pressure.image_pressure,
                    content,
                    oversize_token_threshold,
                );
                match action {
                    UserAction::TruncateOversized => {
                        let before_tokens = content_tokens(content);
                        let max_lines = ctx.config.tool_output_max_lines;
                        let combined_text = content
                            .iter()
                            .filter_map(|c| match c {
                                Content::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        if combined_text.is_empty() {
                            result.push(msg);
                            continue;
                        }

                        let mut new_content = Vec::new();
                        new_content.push(Content::Text {
                            text: truncate_text_head_tail(&combined_text, max_lines),
                        });
                        new_content.extend(content.iter().filter_map(|c| match c {
                            Content::Text { .. } => None,
                            other => Some(other.clone()),
                        }));

                        let after_tokens = content_tokens(&new_content);
                        if after_tokens < before_tokens {
                            running_tokens -= before_tokens - after_tokens;
                            actions.push(CompactionAction {
                                index: idx,
                                tool_name: "user".into(),
                                method: CompactionMethod::OversizeCapped,
                                before_tokens,
                                after_tokens,
                                end_index: None,
                                related_count: None,
                            });
                        }
                        result.push(AgentMessage::Llm(Message::User {
                            content: new_content,
                            timestamp: *timestamp,
                        }));
                        continue;
                    }
                    UserAction::StripImages => {
                        let before_tokens = content_tokens(content);
                        let stripped: Vec<Content> = content
                            .iter()
                            .map(|c| match c {
                                Content::Image { .. } => Content::Text {
                                    text: "[image]".into(),
                                },
                                other => other.clone(),
                            })
                            .collect();
                        let after_tokens = content_tokens(&stripped);
                        if before_tokens > after_tokens {
                            running_tokens -= before_tokens - after_tokens;
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
                        result.push(AgentMessage::Llm(Message::User {
                            content: stripped,
                            timestamp: *timestamp,
                        }));
                        continue;
                    }
                    UserAction::Keep => {}
                }
            }

            // --- Tool results ---
            if let AgentMessage::Llm(Message::ToolResult {
                ref tool_call_id,
                ref tool_name,
                ref content,
                is_error,
                timestamp,
                ref retention,
            }) = msg
            {
                let tokens = content_tokens(content);
                let tp = tool_policy(tool_name, ctx.config.tool_output_max_lines);
                let decision = classify_tool_result(ToolDecisionInput {
                    tokens,
                    is_recent,
                    running_tokens,
                    budget: ctx.config.compact_target(),
                    oversize_token_threshold,
                    policy: &tp,
                });

                // PLACEHOLDER_SHRINK_TOOL_DECISIONS
                match decision {
                    ToolShrinkDecision::OversizeCap => {
                        let max_lines = tp.oversize_max_lines;
                        let truncated = truncate_content(
                            content,
                            tool_name,
                            tool_call_id,
                            &tool_call_index,
                            max_lines,
                            tp.prefer_outline,
                        );
                        let after_tokens = content_tokens(&truncated);
                        if after_tokens < tokens {
                            running_tokens -= tokens - after_tokens;
                            actions.push(CompactionAction {
                                index: idx,
                                tool_name: tool_name.clone(),
                                method: CompactionMethod::OversizeCapped,
                                before_tokens: tokens,
                                after_tokens,
                                end_index: None,
                                related_count: None,
                            });
                            result.push(AgentMessage::Llm(Message::ToolResult {
                                tool_call_id: tool_call_id.clone(),
                                tool_name: tool_name.clone(),
                                content: truncated,
                                is_error,
                                timestamp,
                                retention: *retention,
                            }));
                            continue;
                        }
                    }
                    ToolShrinkDecision::AgeClear => {
                        let marker = format!("[{tool_name} result cleared — {tokens} tokens]");
                        let replacement = vec![Content::Text { text: marker }];
                        let after_tokens = content_tokens(&replacement);
                        running_tokens -= tokens.saturating_sub(after_tokens);
                        actions.push(CompactionAction {
                            index: idx,
                            tool_name: tool_name.clone(),
                            method: CompactionMethod::AgeCleared,
                            before_tokens: tokens,
                            after_tokens,
                            end_index: None,
                            related_count: None,
                        });
                        result.push(AgentMessage::Llm(Message::ToolResult {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                            content: replacement,
                            is_error,
                            timestamp,
                            retention: *retention,
                        }));
                        continue;
                    }
                    // PLACEHOLDER_SHRINK_REMAINING
                    ToolShrinkDecision::NormalTrunc => {
                        let max_lines = tp.normal_max_lines;
                        let truncated = truncate_content(
                            content,
                            tool_name,
                            tool_call_id,
                            &tool_call_index,
                            max_lines,
                            tp.prefer_outline,
                        );
                        let after_tokens = content_tokens(&truncated);
                        if after_tokens < tokens {
                            running_tokens -= tokens - after_tokens;
                            actions.push(CompactionAction {
                                index: idx,
                                tool_name: tool_name.clone(),
                                method: CompactionMethod::HeadTail,
                                before_tokens: tokens,
                                after_tokens,
                                end_index: None,
                                related_count: None,
                            });
                            result.push(AgentMessage::Llm(Message::ToolResult {
                                tool_call_id: tool_call_id.clone(),
                                tool_name: tool_name.clone(),
                                content: truncated,
                                is_error,
                                timestamp,
                                retention: *retention,
                            }));
                            continue;
                        }
                    }
                    ToolShrinkDecision::Keep => {}
                }
            }

            result.push(msg);
        }

        PassResult {
            messages: result,
            actions,
        }
    }
}

fn oversize_threshold(ctx: &PassContext<'_>) -> usize {
    let budget_threshold =
        (ctx.config.budget_tokens as f64 * ctx.config.oversize_budget_ratio) as usize;
    ctx.config.oversize_abs_tokens.min(budget_threshold.max(1))
}
