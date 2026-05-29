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

    // 1. Pinned head: keep_first messages, expanded to complete turn boundary.
    let head_end = expand_head(messages, config.keep_first);

    // 2. Retained tail: walk backwards accumulating tokens until budget exhausted.
    let tail_start = find_retention_boundary(messages, head_end, config);

    // 3. Evict zone = head_end..tail_start
    if head_end >= tail_start {
        return None; // Nothing to evict
    }

    // 4. Detect split turn
    let split_turn = detect_split_turn(messages, tail_start, head_end);

    Some(CompactionPlan {
        pinned_head: 0..head_end,
        evict_zone: head_end..tail_start,
        retained_tail: tail_start..len,
        split_turn,
    })
}

/// Expand head boundary to include complete turns (don't orphan tool results).
fn expand_head(messages: &[AgentMessage], keep_first: usize) -> usize {
    let mut end = keep_first.min(messages.len());
    let limit = messages.len(); // safety cap

    while end < limit {
        match &messages[end] {
            // If previous message is an assistant with tool calls, include trailing results.
            AgentMessage::Llm(Message::ToolResult { .. }) => {
                end += 1;
            }
            _ => break,
        }
    }

    end
}

/// Walk backwards from the end, accumulating tokens. Stop when we've reached
/// `keep_recent_tokens` AND `keep_recent_min` messages. Then snap to a valid
/// cut point.
fn find_retention_boundary(
    messages: &[AgentMessage],
    floor: usize,
    config: &CompactionConfig,
) -> usize {
    let len = messages.len();
    let mut accumulated_tokens = 0usize;
    let mut candidate = len; // default: keep everything (no eviction)

    for i in (floor..len).rev() {
        accumulated_tokens += message_tokens(&messages[i]);

        let enough_tokens = accumulated_tokens >= config.keep_recent_tokens;
        let enough_messages = len - i >= config.keep_recent_min;

        if enough_tokens && enough_messages {
            // Snap forward to a valid cut point (user or assistant boundary).
            candidate = snap_forward_to_cut(messages, i, len);
            break;
        }
    }

    // Ensure we don't cut below the floor.
    candidate.max(floor)
}

/// Find the nearest valid cut point at or after `start`.
/// Valid cut points: user messages or assistant messages (never tool results).
fn snap_forward_to_cut(messages: &[AgentMessage], start: usize, end: usize) -> usize {
    for (i, msg) in messages[start..end].iter().enumerate() {
        if is_valid_cut_point(msg) {
            return start + i;
        }
    }
    end
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
fn detect_split_turn(
    messages: &[AgentMessage],
    tail_start: usize,
    floor: usize,
) -> Option<SplitTurn> {
    // If tail_start is at or beyond the end, no split possible.
    if tail_start >= messages.len() {
        return None;
    }

    // If tail starts at a user message, no split.
    if is_user(&messages[tail_start]) {
        return None;
    }

    // Walk backwards to find the user message that started this turn.
    for i in (floor..tail_start).rev() {
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
