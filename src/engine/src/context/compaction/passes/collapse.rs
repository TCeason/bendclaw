//! Collapse pass — structural summarization of old assistant turns.
//!
//! Replaces old assistant messages + their trailing tool results with
//! a short `[Summary]` line. No LLM involved — purely structural.

use crate::context::compaction::pass::Pass;
use crate::context::compaction::pass::PassContext;
use crate::context::compaction::pass::PassLevel;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::types::CompactionAction;
use crate::context::compaction::types::CompactionMethod;
use crate::context::tokens::message_tokens;
use crate::types::*;

pub struct Collapse;

impl Pass for Collapse {
    fn level(&self) -> PassLevel {
        PassLevel::Collapse
    }

    fn should_run(&self, ctx: &PassContext<'_>) -> bool {
        ctx.pressure.message_tokens > ctx.config.compact_trigger()
    }

    fn run(&self, messages: Vec<AgentMessage>, ctx: &PassContext<'_>) -> PassResult {
        let len = messages.len();
        if len <= ctx.config.keep_recent {
            return PassResult {
                messages,
                actions: vec![],
            };
        }

        let boundary = len - ctx.config.keep_recent;
        let compact_target = ctx.config.compact_target();
        let mut result = Vec::new();
        let mut actions = Vec::new();
        let mut running_tokens = ctx.pressure.message_tokens;

        let mut i = 0;
        while i < boundary {
            if running_tokens <= compact_target {
                while i < boundary {
                    result.push(messages[i].clone());
                    i += 1;
                }
                break;
            }

            let msg = &messages[i];
            match msg {
                AgentMessage::Llm(Message::Assistant { content, .. }) => {
                    let turn_start = i;
                    let before_tokens = message_tokens(msg);

                    // Extract tool names (deduplicated)
                    let mut tool_names: Vec<String> = Vec::new();
                    let mut seen_tools = std::collections::HashSet::new();
                    for c in content {
                        if let Content::ToolCall { name, .. } = c {
                            if seen_tools.insert(name.clone()) {
                                tool_names.push(name.clone());
                            }
                        }
                    }

                    // Extract short text fragments
                    let text_parts: Vec<&str> = content
                        .iter()
                        .filter_map(|c| match c {
                            Content::Text { text } if text.len() <= 200 && !is_filler(text) => {
                                Some(text.as_str())
                            }
                            _ => None,
                        })
                        .take(3)
                        .collect();

                    // Build summary
                    let summary = if !tool_names.is_empty() {
                        let tools_part = if tool_names.len() <= 3 {
                            tool_names.join(", ")
                        } else {
                            format!("[Assistant used {} tool(s)]", tool_names.len())
                        };
                        if !text_parts.is_empty() {
                            format!("[Summary] {} — \"{}\"", tools_part, text_parts.join(" "))
                        } else {
                            format!("[Summary] {}", tools_part)
                        }
                    } else if !text_parts.is_empty() {
                        format!("[Summary] {}", text_parts.join(" "))
                    } else {
                        "[Summary] [Assistant response]".into()
                    };

                    let summary_msg = AgentMessage::Llm(Message::Assistant {
                        content: vec![Content::Text { text: summary }],
                        stop_reason: StopReason::Stop,
                        model: "compaction".into(),
                        provider: "evot".into(),
                        usage: Usage::default(),
                        timestamp: now_ms().unwrap_or(0),
                        error_message: None,
                        response_id: None,
                    });
                    let after_tokens = message_tokens(&summary_msg);

                    // Count trailing tool results
                    let mut peek = i + 1;
                    let mut tool_result_count: usize = 0;
                    let mut tool_result_tokens: usize = 0;
                    while peek < boundary {
                        if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[peek] {
                            tool_result_tokens += message_tokens(&messages[peek]);
                            tool_result_count += 1;
                            peek += 1;
                        } else {
                            break;
                        }
                    }

                    let total_before = before_tokens + tool_result_tokens;

                    if after_tokens < total_before {
                        running_tokens -= total_before - after_tokens;
                        result.push(summary_msg);
                        i = peek;

                        actions.push(CompactionAction {
                            index: turn_start,
                            tool_name: "assistant".into(),
                            method: CompactionMethod::TurnCollapsed,
                            before_tokens: total_before,
                            after_tokens,
                            end_index: None,
                            related_count: Some(tool_result_count),
                        });
                    } else {
                        result.push(msg.clone());
                        i += 1;
                        while i < boundary {
                            if let AgentMessage::Llm(Message::ToolResult { .. }) = &messages[i] {
                                result.push(messages[i].clone());
                                i += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    continue;
                }
                AgentMessage::Llm(Message::ToolResult { .. }) => {
                    // Orphaned tool result — keep as-is
                    result.push(msg.clone());
                }
                _ => {
                    result.push(msg.clone());
                }
            }
            i += 1;
        }

        result.extend_from_slice(&messages[boundary..]);

        PassResult {
            messages: result,
            actions,
        }
    }
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

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}
