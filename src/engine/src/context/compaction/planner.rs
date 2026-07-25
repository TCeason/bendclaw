//! Planner — determines the cut point and zones for compaction.
//!
//! Given messages and config, produces a `CompactionPlan` that describes
//! which messages to keep, which to evict, and whether a turn is split.

use super::config::CompactionConfig;
use super::types::CompactionPlan;
use super::types::SplitTurn;
use crate::context::tokens::message_tokens;
use crate::types::AgentMessage;
use crate::types::Message;

/// Plan a compaction. Returns `None` if there's nothing meaningful to evict.
pub fn plan(messages: &[AgentMessage], config: &CompactionConfig) -> Option<CompactionPlan> {
    let len = messages.len();
    if len == 0 {
        return None;
    }

    // 1. Retained tail: walk backwards accumulating tokens until budget exhausted.
    let tail_start = find_retention_boundary(messages, config);

    // 2. Evict zone = everything before the tail. Mirrors pi, which pins no
    //    head: the summary represents all evicted history.
    if tail_start == 0 {
        return None; // Nothing to evict
    }

    // 3. Detect split turn
    let split_turn = detect_split_turn(messages, tail_start);

    Some(CompactionPlan {
        evict_zone: 0..tail_start,
        retained_tail: tail_start..len,
        split_turn,
    })
}

/// Walk backwards from the end, accumulating tokens until `keep_recent_tokens`
/// is reached, then snap to a valid cut point. Mirrors pi's `findCutPoint`: the
/// token budget is the only retention condition.
fn find_retention_boundary(messages: &[AgentMessage], config: &CompactionConfig) -> usize {
    let len = messages.len();
    let Some(first_cut) = snap_forward_to_cut(messages, 0, len) else {
        return 0;
    };
    let mut accumulated_tokens = 0usize;
    let mut candidate = first_cut; // all history fits means keep everything

    for i in (0..len).rev() {
        let tokens = message_tokens(&messages[i]);
        if tokens == 0 {
            continue;
        }
        accumulated_tokens += tokens;

        if accumulated_tokens >= config.keep_recent_tokens {
            // pi falls back to the first valid cut point when no valid point
            // exists at or after the message that crossed the budget.
            candidate = snap_forward_to_cut(messages, i, len).unwrap_or(first_cut);
            break;
        }
    }

    // Keep adjacent UI-only metadata with the retained tail, matching pi's
    // backwards scan over entries that produce no context messages.
    while candidate > 0 && matches!(messages[candidate - 1], AgentMessage::Extension(_)) {
        candidate -= 1;
    }
    candidate
}

/// Find the nearest valid cut point at or after `start`.
/// Valid cut points: user messages or assistant messages (never tool results).
fn snap_forward_to_cut(messages: &[AgentMessage], start: usize, end: usize) -> Option<usize> {
    messages[start..end]
        .iter()
        .position(is_valid_cut_point)
        .map(|offset| start + offset)
}

/// A valid cut point is a user or assistant message (not a tool result).
fn is_valid_cut_point(msg: &AgentMessage) -> bool {
    matches!(
        msg,
        AgentMessage::Llm(Message::User { .. }) | AgentMessage::Llm(Message::Assistant { .. })
    )
}

/// Detect if the cut point splits a turn (i.e., retained_tail starts at an
/// assistant message rather than a user message).
fn detect_split_turn(messages: &[AgentMessage], tail_start: usize) -> Option<SplitTurn> {
    // If tail_start is at or beyond the end, no split possible.
    if tail_start >= messages.len() {
        return None;
    }

    // If tail starts at a user message, no split.
    if is_user(&messages[tail_start]) {
        return None;
    }

    // Walk backwards to find the user message that started this turn.
    for i in (0..tail_start).rev() {
        if is_user(&messages[i]) {
            return Some(SplitTurn {
                turn_start: i,
                cut_at: tail_start,
            });
        }
    }

    None
}

fn is_user(msg: &AgentMessage) -> bool {
    matches!(msg, AgentMessage::Llm(Message::User { .. }))
}
