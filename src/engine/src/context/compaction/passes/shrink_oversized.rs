//! Shrink oversized tool results using policy-driven truncation.
//!
//! **Always-on** — runs unconditionally regardless of budget.
//!
//! Strategy: `ToolPolicy` from `policy::tool_policy()`.
//! Global thresholds control *when* a result is oversized; per-tool policy
//! controls *how* it is handled.
//!
//! Three tiers, evaluated in priority order per `ToolResult`:
//!
//!   Tier 1 — AgeEvict: old + large → clear to marker
//!   Tier 2 — OversizeCap: individually too large → aggressive truncation
//!   Tier 3 — NormalTrunc: over budget → try truncating all tool results
//!
//! `over_budget` is computed once at pass entry and held fixed for the
//! entire pass to avoid unstable behaviour during iteration.

use std::collections::HashMap;

use crate::context::compaction::compact::CompactionAction;
use crate::context::compaction::compact::CompactionMethod;
use crate::context::compaction::outline;
use crate::context::compaction::pass::CompactionContext;
use crate::context::compaction::pass::CompactionPass;
use crate::context::compaction::pass::PassResult;
use crate::context::compaction::policy::tool_policy;
use crate::context::tokens::content_tokens;
use crate::context::tokens::total_tokens;
use crate::types::*;

pub struct ShrinkOversizedToolResults;

impl CompactionPass for ShrinkOversizedToolResults {
    fn name(&self) -> &str {
        "ShrinkOversizedToolResults"
    }

    fn run(&self, messages: Vec<AgentMessage>, ctx: &CompactionContext) -> PassResult {
        let over_budget = total_tokens(&messages) > ctx.budget;
        let tool_call_index = build_tool_call_index(&messages);
        let len = messages.len();
        let recent_boundary = len.saturating_sub(ctx.keep_recent);

        let oversize_token_threshold = ctx
            .policy
            .oversize_abs_tokens
            .max((ctx.budget as f64 * ctx.policy.oversize_budget_ratio) as usize);

        let mut actions = Vec::new();

        let result = messages
            .into_iter()
            .enumerate()
            .map(|(idx, msg)| {
                let is_tool_result = matches!(&msg, AgentMessage::Llm(Message::ToolResult { .. }));
                if !is_tool_result {
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
                    let tokens = content_tokens(&content);
                    let is_recent = idx >= recent_boundary;
                    let tp = tool_policy(&tool_name);

                    // Tier 1: AgeEvict — old result exceeding age threshold (budget-gated)
                    if over_budget && !is_recent {
                        if let Some(threshold) = tp.age_clear_threshold {
                            if tokens > threshold {
                                let marker =
                                    format!("[{tool_name} result cleared — {tokens} tokens]");
                                let replacement = vec![Content::Text { text: marker }];
                                let after_tokens = content_tokens(&replacement);

                                actions.push(CompactionAction {
                                    index: idx,
                                    tool_name: tool_name.clone(),
                                    method: CompactionMethod::AgeCleared,
                                    before_tokens: tokens,
                                    after_tokens,
                                    end_index: None,
                                    related_count: None,
                                });

                                return AgentMessage::Llm(Message::ToolResult {
                                    tool_call_id,
                                    tool_name,
                                    content: replacement,
                                    is_error,
                                    timestamp,
                                    retention,
                                });
                            }
                        }
                    }

                    // Tier 2: OversizeCap — individually too large
                    if tokens > oversize_token_threshold {
                        let max_lines = tp.oversize_max_lines.min(ctx.tool_output_max_lines);
                        let truncated = truncate_content(
                            &content,
                            &tool_name,
                            &tool_call_id,
                            &tool_call_index,
                            max_lines,
                            tp.prefer_outline,
                        );
                        let after_tokens = content_tokens(&truncated);

                        if after_tokens < tokens {
                            actions.push(CompactionAction {
                                index: idx,
                                tool_name: tool_name.clone(),
                                method: CompactionMethod::OversizeCapped,
                                before_tokens: tokens,
                                after_tokens,
                                end_index: None,
                                related_count: None,
                            });

                            return AgentMessage::Llm(Message::ToolResult {
                                tool_call_id,
                                tool_name,
                                content: truncated,
                                is_error,
                                timestamp,
                                retention,
                            });
                        }
                    }

                    // Tier 3: NormalTrunc — over budget → try truncating all tool results
                    if over_budget {
                        let max_lines = tp.normal_max_lines.min(ctx.tool_output_max_lines);
                        let truncated = truncate_content(
                            &content,
                            &tool_name,
                            &tool_call_id,
                            &tool_call_index,
                            max_lines,
                            tp.prefer_outline,
                        );
                        let after_tokens = content_tokens(&truncated);

                        if after_tokens < tokens {
                            let method = detect_method(&content, &truncated);

                            actions.push(CompactionAction {
                                index: idx,
                                tool_name: tool_name.clone(),
                                method,
                                before_tokens: tokens,
                                after_tokens,
                                end_index: None,
                                related_count: None,
                            });

                            return AgentMessage::Llm(Message::ToolResult {
                                tool_call_id,
                                tool_name,
                                content: truncated,
                                is_error,
                                timestamp,
                                retention,
                            });
                        }
                    }

                    // No truncation needed
                    AgentMessage::Llm(Message::ToolResult {
                        tool_call_id,
                        tool_name,
                        content,
                        is_error,
                        timestamp,
                        retention,
                    })
                } else {
                    msg
                }
            })
            .collect();

        PassResult {
            messages: result,
            actions,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an index from tool_call_id → ToolCall arguments.
fn build_tool_call_index(messages: &[AgentMessage]) -> HashMap<String, serde_json::Value> {
    let mut index = HashMap::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = msg {
            for c in content {
                if let Content::ToolCall { id, arguments, .. } = c {
                    index.insert(id.clone(), arguments.clone());
                }
            }
        }
    }
    index
}

/// Byte limit for tool result text after line truncation.
/// Catches cases where individual lines are very long (minified JSON/HTML).
const COMPACTION_MAX_BYTES: usize = 15_000;

/// Truncate content blocks using outline (if preferred) or head-tail,
/// then apply a byte cap to catch long single-line content.
fn truncate_content(
    content: &[Content],
    _tool_name: &str,
    tool_call_id: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    max_lines: usize,
    prefer_outline: bool,
) -> Vec<Content> {
    content
        .iter()
        .map(|c| match c {
            Content::Text { text } => {
                let truncated = if prefer_outline {
                    try_outline_or_truncate(text, tool_call_index, tool_call_id, max_lines)
                } else {
                    truncate_text_head_tail(text, max_lines)
                };
                // Second pass: byte cap (line truncation alone may not be
                // enough when individual lines are very long).
                let truncated =
                    crate::tools::validation::truncate_tool_text(&truncated, COMPACTION_MAX_BYTES);
                Content::Text { text: truncated }
            }
            other => other.clone(),
        })
        .collect()
}

/// Try tree-sitter outline for read_file, fall back to head-tail.
fn try_outline_or_truncate(
    text: &str,
    tool_call_index: &HashMap<String, serde_json::Value>,
    tool_call_id: &str,
    max_lines: usize,
) -> String {
    // Extract file path and extension from the tool call arguments
    if let Some(args) = tool_call_index.get(tool_call_id) {
        if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
            let ext = std::path::Path::new(path_str)
                .extension()
                .and_then(|e| e.to_str());
            if let Some(ext) = ext {
                if let Some(outlined) =
                    outline::extract_outline_from_read_file_output(text, ext, path_str)
                {
                    // Use outline only if it saves at least 10%
                    let threshold = text.len() / 10;
                    if outlined.len() + threshold < text.len() {
                        return outlined;
                    }
                }
            }
        }
    }

    truncate_text_head_tail(text, max_lines)
}

/// Truncate text keeping first N/2 and last N/2 lines.
pub fn truncate_text_head_tail(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }

    let head = max_lines / 2;
    let tail = max_lines - head;
    let omitted = lines.len() - head - tail;

    let mut result = lines[..head].join("\n");
    result.push_str(&format!("\n\n[... {} lines truncated ...]\n\n", omitted));
    result.push_str(&lines[lines.len() - tail..].join("\n"));
    result
}

/// Detect whether outline or head-tail was used by checking content.
fn detect_method(original: &[Content], truncated: &[Content]) -> CompactionMethod {
    for t in truncated {
        if let Content::Text { text } = t {
            if text.contains("[Structural outline of") {
                return CompactionMethod::Outline;
            }
            if text.contains("[... ") && text.contains(" lines truncated ...]") {
                return CompactionMethod::HeadTail;
            }
        }
    }
    // If content changed but no marker detected, it was still capped
    let orig_len: usize = original
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    let trunc_len: usize = truncated
        .iter()
        .map(|c| match c {
            Content::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    if trunc_len < orig_len {
        CompactionMethod::OversizeCapped
    } else {
        CompactionMethod::HeadTail
    }
}
