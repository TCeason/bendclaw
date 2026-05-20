//! Message eviction transforms.
//!
//! These functions only perform the selected eviction transform. Pressure
//! classification and mode selection live in `pressure.rs` and `levels.rs`.
//!
//! Token eviction keeps the beginning and recent tail, drops stale middle
//! content, and falls back to tail-first retention when needed.

use crate::context::compaction::compact::CompactionAction;
use crate::context::compaction::compact::CompactionMethod;
use crate::context::compaction::phase::PhaseContext;
use crate::context::compaction::phase::PhaseResult;
use crate::context::tokens::message_tokens;
use crate::context::tokens::total_tokens;
use crate::types::*;

/// Maximum number of evicted user texts to include in the marker.
const MAX_EVICTED_USER_TEXTS: usize = 5;

pub fn drop_to_message_target(messages: Vec<AgentMessage>, ctx: &PhaseContext) -> PhaseResult {
    let len = messages.len();
    let target_len = message_limit_target(ctx, len);

    if len <= target_len {
        return PhaseResult {
            messages,
            actions: Vec::new(),
        };
    }

    let first_end = ctx.bounds.keep_first.min(len);
    let recent_len = ctx
        .bounds
        .keep_recent
        .min(target_len.saturating_sub(first_end + 1));
    let recent_start = len.saturating_sub(recent_len);
    let removable = recent_start.saturating_sub(first_end);
    let remove_count = len.saturating_sub(target_len).min(removable);

    if remove_count == 0 {
        return PhaseResult {
            messages,
            actions: Vec::new(),
        };
    }

    let drop_indices = choose_message_limit_drops(
        &messages,
        first_end,
        recent_start,
        remove_count,
        message_limit_remove_cap(
            len,
            ctx.bounds.max_messages,
            ctx.bounds.message_limit_target_pct.clamp(1, 100) as usize,
        ),
    );
    if drop_indices.is_empty() {
        return PhaseResult {
            messages,
            actions: Vec::new(),
        };
    }

    let dropped_tokens: usize = drop_indices
        .iter()
        .map(|&idx| message_tokens(&messages[idx]))
        .sum();

    // Collect user message texts from evicted messages so the marker can
    // list them as "completed tasks" — prevents the model from re-orienting
    // to old retained user messages whose responses were evicted.
    // Filter out internal messages and cap at 5 to avoid bloating the marker.
    let evicted_user_texts: Vec<String> = drop_indices
        .iter()
        .filter_map(|&idx| {
            if let AgentMessage::Llm(Message::User { content, .. }) = &messages[idx] {
                content.iter().find_map(|c| {
                    if let Content::Text { text } = c {
                        let t = text.trim();
                        if !t.is_empty() && !is_internal_user_text(t) {
                            return Some(t.to_string());
                        }
                    }
                    None
                })
            } else {
                None
            }
        })
        .take(MAX_EVICTED_USER_TEXTS)
        .collect();

    // Collect user texts from the keep_first region so the marker can
    // explicitly label them as completed — prevents the model from
    // re-orienting to old tasks that remain at the top of context.
    let retained_early_user_texts: Vec<String> = messages[..first_end]
        .iter()
        .filter_map(|msg| {
            if let AgentMessage::Llm(Message::User { content, .. }) = msg {
                content.iter().find_map(|c| {
                    if let Content::Text { text } = c {
                        let t = text.trim();
                        if !t.is_empty() && !is_internal_user_text(t) {
                            return Some(t.to_string());
                        }
                    }
                    None
                })
            } else {
                None
            }
        })
        .collect();

    let marker = super::super::super::marker::build_marker_with_evicted(
        &messages,
        drop_indices.len(),
        dropped_tokens,
        &evicted_user_texts,
        &retained_early_user_texts,
    );
    let marker_tokens = message_tokens(&marker);

    // Always insert at least a minimal marker so the model knows messages
    // were removed. Use the full marker when it fits within the freed budget;
    // fall back to a minimal one-liner otherwise.
    let effective_marker = if marker_tokens < dropped_tokens {
        marker
    } else {
        super::super::super::marker::build_minimal_marker(drop_indices.len())
    };
    let effective_marker_tokens = message_tokens(&effective_marker);

    let mut result = Vec::with_capacity(target_len);
    let mut inserted_marker = false;
    let mut drop_pos = 0;
    for (idx, msg) in messages.into_iter().enumerate() {
        let should_drop = drop_pos < drop_indices.len() && drop_indices[drop_pos] == idx;
        if should_drop {
            if !inserted_marker {
                result.push(effective_marker.clone());
                inserted_marker = true;
            }
            drop_pos += 1;
        } else {
            result.push(msg);
        }
    }

    let first_dropped = drop_indices[0];
    let last_dropped = drop_indices[drop_indices.len() - 1];
    let action = CompactionAction {
        index: first_dropped,
        tool_name: "messages".into(),
        method: CompactionMethod::MessagesEvicted,
        before_tokens: dropped_tokens.max(effective_marker_tokens),
        after_tokens: effective_marker_tokens,
        end_index: Some(last_dropped),
        related_count: Some(drop_indices.len()),
    };

    PhaseResult {
        messages: result,
        actions: vec![action],
    }
}

fn message_limit_target(ctx: &PhaseContext, current_len: usize) -> usize {
    if ctx.bounds.max_messages == 0 {
        return 0;
    }

    let pct = ctx.bounds.message_limit_target_pct.clamp(1, 100) as usize;
    let minimum = ctx
        .bounds
        .keep_first
        .saturating_add(ctx.bounds.keep_recent)
        .saturating_add(1);
    let pct_target = ctx
        .bounds
        .max_messages
        .saturating_mul(pct)
        .saturating_add(99)
        / 100;

    pct_target
        .max(minimum)
        .min(ctx.bounds.max_messages)
        .min(current_len)
}

fn message_limit_remove_cap(current_len: usize, max_messages: usize, target_pct: usize) -> usize {
    if max_messages == 0 || current_len <= max_messages {
        return 0;
    }

    let target_len = max_messages
        .saturating_mul(target_pct.clamp(1, 100))
        .saturating_add(99)
        / 100;
    let requested = current_len.saturating_sub(target_len);

    if max_messages < 100 || target_pct >= 100 {
        return usize::MAX;
    }

    let soft_cap = max_messages.saturating_add(9) / 10;
    requested.min(
        current_len
            .saturating_sub(max_messages)
            .saturating_add(soft_cap),
    )
}

/// Choose which messages to drop from the middle section.
///
/// Priority:
///   0. Drop tool-call rounds (assistant(ToolUse) + following ToolResults) first.
///      These are the biggest token consumers and their results are already
///      reflected in the final assistant response. Only dropped when the same
///      turn still has a final assistant response retained.
///   1. Drop complete stale turns (user + all responses) oldest-first.
///      Removes the old user prompt together with its answer so it cannot
///      look like an unfinished task.
///   2. Drop orphan assistant/tool runs that are fully in the stale range.
///
/// Drops are always in complete spans — never truncates a turn midway.
/// Oversized spans are skipped so message-count cleanup cannot erase most of a session.
fn choose_message_limit_drops(
    messages: &[AgentMessage],
    start: usize,
    end: usize,
    remove_count: usize,
    remove_cap: usize,
) -> Vec<usize> {
    // Identify turns in the full message list, then only emit spans fully contained in
    // the droppable range. This avoids keep_first/keep_recent slicing through a turn.
    let mut spans: Vec<Span> = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        if is_user_message(&messages[i]) {
            let turn_start = i;
            let mut j = i + 1;
            let mut tool_spans: Vec<(usize, usize)> = Vec::new();
            while j < messages.len() && is_assistant_or_tool(&messages[j]) {
                if is_tool_use_assistant(&messages[j]) {
                    let span_start = j;
                    j += 1;
                    while j < messages.len() && is_tool_result(&messages[j]) {
                        j += 1;
                    }
                    tool_spans.push((span_start, j));
                } else {
                    j += 1;
                }
            }
            let turn_end = j;

            // Priority 0: tool rounds (only if turn has a final assistant)
            let has_final = (turn_start + 1..turn_end).any(|k| is_final_assistant(&messages[k]));
            if has_final {
                for (ts, te) in &tool_spans {
                    if *ts >= start && *te <= end {
                        spans.push(Span {
                            indices: (*ts..*te).collect(),
                            priority: 0,
                        });
                    }
                }
            }

            // Priority 1: complete turn
            if turn_start >= start && turn_end <= end {
                spans.push(Span {
                    indices: (turn_start..turn_end).collect(),
                    priority: 1,
                });
            }

            i = turn_end;
        } else if is_assistant_or_tool(&messages[i]) {
            let span_start = i;
            while i < messages.len() && is_assistant_or_tool(&messages[i]) {
                i += 1;
            }
            // Priority 2: orphan assistant/tool runs
            if span_start >= start && i <= end {
                spans.push(Span {
                    indices: (span_start..i).collect(),
                    priority: 2,
                });
            }
        } else {
            i += 1;
        }
    }

    spans.sort_by_key(|s| s.priority);

    let mut dropped: Vec<bool> = vec![false; messages.len()];
    let mut total_dropped = 0;

    for span in &spans {
        if total_dropped >= remove_count {
            break;
        }
        let new_indices: Vec<usize> = span
            .indices
            .iter()
            .filter(|&&idx| !dropped[idx])
            .copied()
            .collect();
        if new_indices.is_empty() || total_dropped >= remove_count {
            continue;
        }
        if total_dropped + new_indices.len() > remove_cap {
            continue;
        }
        for &idx in &new_indices {
            dropped[idx] = true;
        }
        total_dropped += new_indices.len();
    }

    dropped
        .iter()
        .enumerate()
        .filter(|(_, &d)| d)
        .map(|(idx, _)| idx)
        .collect()
}

struct Span {
    indices: Vec<usize>,
    priority: u8, // 0 = tool rounds, 1 = complete turns, 2 = orphan runs
}

fn is_user_message(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::Llm(Message::User { .. }))
}

fn is_tool_use_assistant(message: &AgentMessage) -> bool {
    matches!(
        message,
        AgentMessage::Llm(Message::Assistant {
            stop_reason: StopReason::ToolUse,
            ..
        })
    )
}

fn is_final_assistant(message: &AgentMessage) -> bool {
    matches!(
        message,
        AgentMessage::Llm(Message::Assistant {
            stop_reason: StopReason::Stop,
            ..
        })
    )
}

fn is_tool_result(message: &AgentMessage) -> bool {
    matches!(message, AgentMessage::Llm(Message::ToolResult { .. }))
}

fn is_assistant_or_tool(message: &AgentMessage) -> bool {
    matches!(
        message,
        AgentMessage::Llm(Message::Assistant { .. } | Message::ToolResult { .. })
    )
}

pub fn drop_to_token_target(messages: Vec<AgentMessage>, ctx: &PhaseContext) -> PhaseResult {
    let len = messages.len();
    let target_messages = ctx
        .bounds
        .keep_first
        .saturating_add(ctx.bounds.keep_recent)
        .saturating_add(1)
        .max(1);
    let before_tokens = total_tokens(&messages);
    if len <= target_messages && before_tokens <= ctx.budget.compact_target {
        return PhaseResult {
            messages,
            actions: Vec::new(),
        };
    }

    let first_end = ctx.bounds.keep_first.min(len);
    let recent_start = len.saturating_sub(ctx.bounds.keep_recent);
    let token_target = ctx.budget.max_tokens;

    if first_end >= recent_start {
        let result = keep_within_budget(&messages, first_end, token_target);
        let after_tokens = total_tokens(&result);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 && after_tokens <= before_tokens {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::MessagesEvicted,
                before_tokens,
                after_tokens,
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PhaseResult {
            messages: result,
            actions,
        };
    }

    let first_msgs = &messages[..first_end];
    let recent_msgs = &messages[recent_start..];
    let removed = recent_start - first_end;

    let dropped_tokens: usize = messages[first_end..recent_start]
        .iter()
        .map(message_tokens)
        .sum();

    // Collect user message texts from evicted range for the marker.
    let evicted_user_texts: Vec<String> = messages[first_end..recent_start]
        .iter()
        .filter_map(|msg| {
            if let AgentMessage::Llm(Message::User { content, .. }) = msg {
                content.iter().find_map(|c| {
                    if let Content::Text { text } = c {
                        let t = text.trim();
                        if !t.is_empty() && !is_internal_user_text(t) {
                            return Some(t.to_string());
                        }
                    }
                    None
                })
            } else {
                None
            }
        })
        .take(MAX_EVICTED_USER_TEXTS)
        .collect();

    // Collect user texts from the keep_first region so the marker can
    // explicitly label them as completed.
    let retained_early_user_texts: Vec<String> = messages[..first_end]
        .iter()
        .filter_map(|msg| {
            if let AgentMessage::Llm(Message::User { content, .. }) = msg {
                content.iter().find_map(|c| {
                    if let Content::Text { text } = c {
                        let t = text.trim();
                        if !t.is_empty() && !is_internal_user_text(t) {
                            return Some(t.to_string());
                        }
                    }
                    None
                })
            } else {
                None
            }
        })
        .collect();

    let marker = super::super::super::marker::build_marker_with_evicted(
        &messages,
        removed,
        dropped_tokens,
        &evicted_user_texts,
        &retained_early_user_texts,
    );
    let marker_tokens = message_tokens(&marker);

    let mut result = first_msgs.to_vec();
    result.push(marker);
    result.extend_from_slice(recent_msgs);

    let result_tokens = total_tokens(&result);
    if result_tokens >= before_tokens {
        let fallback = keep_within_budget(&messages, first_end, token_target);
        let fallback_tokens = total_tokens(&fallback);
        let dropped = len.saturating_sub(fallback.len());
        let actions = if dropped > 0 && fallback_tokens <= before_tokens {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::MessagesEvicted,
                before_tokens,
                after_tokens: fallback_tokens,
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PhaseResult {
            messages: fallback,
            actions,
        };
    }

    if result_tokens > token_target {
        let result = keep_within_budget(&result, first_end, token_target);
        let after_tokens = total_tokens(&result);
        let dropped = len.saturating_sub(result.len());
        let actions = if dropped > 0 && after_tokens <= before_tokens {
            vec![CompactionAction {
                index: 0,
                tool_name: "messages".into(),
                method: CompactionMethod::MessagesEvicted,
                before_tokens,
                after_tokens,
                end_index: None,
                related_count: Some(dropped),
            }]
        } else {
            vec![]
        };
        return PhaseResult {
            messages: result,
            actions,
        };
    }

    let actions = vec![CompactionAction {
        index: first_end,
        tool_name: "messages".into(),
        method: CompactionMethod::MessagesEvicted,
        before_tokens: dropped_tokens.max(marker_tokens),
        after_tokens: marker_tokens,
        end_index: Some(recent_start.saturating_sub(1)),
        related_count: Some(removed),
    }];

    PhaseResult {
        messages: result,
        actions,
    }
}

/// Keep messages within budget using tail-first retention.
///
/// Protects the first `keep_first` messages, then fills the remaining
/// budget from the tail (most-recent-first). Old messages — including
/// compaction summaries accumulated at the front — are naturally dropped.
fn keep_within_budget(
    messages: &[AgentMessage],
    keep_first: usize,
    budget: usize,
) -> Vec<AgentMessage> {
    if messages.is_empty() {
        return Vec::new();
    }

    let protected_end = keep_first.max(1).min(messages.len());
    let protected = &messages[..protected_end];
    let protected_tokens: usize = protected.iter().map(message_tokens).sum();

    if protected_tokens >= budget {
        let first = messages[0].clone();
        let first_tokens = message_tokens(&first);
        if first_tokens > budget {
            if let AgentMessage::Llm(Message::User { content, timestamp }) = first {
                let capped = crate::tools::validation::cap_tool_result_content(content, budget * 4);
                return vec![AgentMessage::Llm(Message::User {
                    content: capped,
                    timestamp,
                })];
            }
        }
        return vec![first];
    }

    let mut remaining = budget - protected_tokens;
    let rest = &messages[protected_end..];

    // Tail-first: most recent messages first. Old summaries naturally
    // fall behind newer messages and are dropped when budget fills.
    let mut tail: Vec<(usize, AgentMessage)> = Vec::new();
    for (offset, msg) in rest.iter().enumerate().rev() {
        let tokens = message_tokens(msg);
        if tokens > remaining {
            continue;
        }
        remaining -= tokens;
        tail.push((protected_end + offset, msg.clone()));
    }
    tail.reverse();

    let kept_tail_indices: Vec<usize> = tail.iter().map(|(idx, _)| *idx).collect();
    let kept_tail_tokens: Vec<usize> = tail.iter().map(|(_, msg)| message_tokens(msg)).collect();
    let mut remaining_tail_tokens = vec![0; kept_tail_tokens.len() + 1];
    for idx in (0..kept_tail_tokens.len()).rev() {
        remaining_tail_tokens[idx] = remaining_tail_tokens[idx + 1] + kept_tail_tokens[idx];
    }
    let kept = protected_end + tail.len();
    let removed = messages.len() - kept;

    let mut result: Vec<AgentMessage> = protected.to_vec();
    if removed > 0 {
        let mut kept_pos = 0;
        let mut pending_removed = 0;
        let mut pending_removed_tokens = 0;
        let mut used_tokens = protected_tokens;
        for (idx, msg) in messages.iter().enumerate().skip(protected_end) {
            let is_kept = kept_pos < kept_tail_indices.len() && kept_tail_indices[kept_pos] == idx;
            if is_kept {
                if pending_removed > 0 {
                    let marker = super::super::super::marker::build_marker(
                        messages,
                        pending_removed,
                        pending_removed_tokens,
                    );
                    let marker_tokens = message_tokens(&marker);
                    if marker_tokens < pending_removed_tokens
                        && used_tokens + marker_tokens + remaining_tail_tokens[kept_pos] <= budget
                    {
                        used_tokens += marker_tokens;
                        result.push(marker);
                    }
                    pending_removed = 0;
                    pending_removed_tokens = 0;
                }
                used_tokens += message_tokens(msg);
                result.push(msg.clone());
                kept_pos += 1;
            } else {
                pending_removed += 1;
                pending_removed_tokens += message_tokens(msg);
            }
        }
        if pending_removed > 0 {
            let marker = super::super::super::marker::build_marker(
                messages,
                pending_removed,
                pending_removed_tokens,
            );
            let marker_tokens = message_tokens(&marker);
            if marker_tokens < pending_removed_tokens && used_tokens + marker_tokens <= budget {
                result.push(marker);
            }
        }
        return result;
    }
    result.extend(tail.into_iter().map(|(_, msg)| msg));
    result
}

/// Returns true for user messages that are internal bookkeeping (system
/// reminders, prior compaction markers) — these should not appear in the
/// "completed tasks" list.
fn is_internal_user_text(text: &str) -> bool {
    text.starts_with("<system-reminder>") || text.starts_with("[Context compacted")
}
