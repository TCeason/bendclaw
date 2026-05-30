//! Session-entry aware compaction primitives.
//!
//! This module is intentionally small and app-agnostic: callers provide a
//! linear stream of entries with stable sequence numbers and already-converted
//! `AgentMessage`s. The planner returns a declarative boundary (`first_kept_seq`)
//! instead of a compacted snapshot.

use std::ops::Range;

use super::types::FileOps;
use crate::context::tokens::message_tokens;
use crate::context::tokens::total_tokens;
use crate::types::AgentMessage;
use crate::types::Message;

/// A message-bearing transcript entry used by the compact planner.
#[derive(Debug, Clone)]
pub struct CompactEntry {
    pub seq: u64,
    pub message: AgentMessage,
}

/// Declarative plan for a session compaction.
#[derive(Debug, Clone)]
pub struct SessionCompactPlan {
    /// Entries summarized into the compact summary.
    pub summarize: Range<usize>,
    /// Optional prefix of the retained current turn that is summarized
    /// separately when the cut point splits a turn.
    pub turn_prefix: Option<Range<usize>>,
    /// Index of the first entry retained verbatim in the future context view.
    pub first_kept: usize,
    pub first_kept_seq: u64,
    pub split_turn: Option<SessionSplitTurn>,
    pub tokens_before: usize,
    pub messages_before: usize,
    pub file_ops: FileOps,
}

#[derive(Debug, Clone)]
pub struct SessionSplitTurn {
    pub turn_start_seq: u64,
    pub cut_seq: u64,
}

/// Plan compaction over a sequence-numbered session context.
///
/// `boundary_seq` is the latest compact entry's first-kept sequence, if any.
/// Entries before it are already represented by the previous summary and are
/// not summarized again.
pub fn plan_session_compaction(
    entries: &[CompactEntry],
    boundary_seq: Option<u64>,
    keep_recent_tokens: usize,
    keep_recent_min_messages: usize,
) -> Option<SessionCompactPlan> {
    if entries.is_empty() {
        return None;
    }

    let boundary = boundary_seq
        .and_then(|seq| entries.iter().position(|entry| entry.seq >= seq))
        .unwrap_or(0);
    if boundary >= entries.len() {
        return None;
    }

    let first_kept = find_first_kept(
        entries,
        boundary,
        keep_recent_tokens,
        keep_recent_min_messages,
    );
    if first_kept <= boundary {
        return None;
    }

    let split_turn = detect_split_turn(entries, boundary, first_kept);
    let summarize_end = split_turn
        .as_ref()
        .map(|s| s.turn_start_index)
        .unwrap_or(first_kept);
    let turn_prefix = split_turn.as_ref().map(|s| s.turn_start_index..first_kept);

    if summarize_end <= boundary && turn_prefix.as_ref().is_none_or(|r| r.is_empty()) {
        return None;
    }

    let file_ops = extract_file_ops_from_entries(&entries[boundary..summarize_end]);
    let tokens_before = total_tokens(
        &entries
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>(),
    );

    let split_turn_public = split_turn.map(|split| SessionSplitTurn {
        turn_start_seq: entries[split.turn_start_index].seq,
        cut_seq: entries[first_kept].seq,
    });

    Some(SessionCompactPlan {
        summarize: boundary..summarize_end,
        turn_prefix,
        first_kept,
        first_kept_seq: entries[first_kept].seq,
        split_turn: split_turn_public,
        tokens_before,
        messages_before: entries.len(),
        file_ops,
    })
}

fn find_first_kept(
    entries: &[CompactEntry],
    boundary: usize,
    keep_recent_tokens: usize,
    keep_recent_min_messages: usize,
) -> usize {
    let mut accumulated = 0usize;
    let mut candidate = entries.len();

    for i in (boundary..entries.len()).rev() {
        accumulated += message_tokens(&entries[i].message);
        let enough_tokens = accumulated >= keep_recent_tokens;
        let enough_messages = entries.len() - i >= keep_recent_min_messages;
        if enough_tokens && enough_messages {
            candidate = snap_forward_to_cut(entries, i, entries.len());
            break;
        }
    }

    candidate.max(boundary).min(entries.len().saturating_sub(1))
}

fn snap_forward_to_cut(entries: &[CompactEntry], start: usize, end: usize) -> usize {
    for (i, entry) in entries.iter().enumerate().take(end).skip(start) {
        if is_valid_cut_point(&entry.message) {
            return i;
        }
    }
    end.saturating_sub(1)
}

fn is_valid_cut_point(msg: &AgentMessage) -> bool {
    matches!(
        msg,
        AgentMessage::Llm(Message::User { .. }) | AgentMessage::Llm(Message::Assistant { .. })
    )
}

fn detect_split_turn(
    entries: &[CompactEntry],
    boundary: usize,
    first_kept: usize,
) -> Option<SplitTurnInternal> {
    if first_kept >= entries.len() || is_user(&entries[first_kept].message) {
        return None;
    }
    for i in (boundary..first_kept).rev() {
        if is_user(&entries[i].message) {
            return Some(SplitTurnInternal {
                turn_start_index: i,
            });
        }
    }
    None
}

#[derive(Debug, Clone)]
struct SplitTurnInternal {
    turn_start_index: usize,
}

fn is_user(msg: &AgentMessage) -> bool {
    matches!(msg, AgentMessage::Llm(Message::User { .. }))
}

fn extract_file_ops_from_entries(entries: &[CompactEntry]) -> FileOps {
    let mut ops = FileOps::default();
    for entry in entries {
        extract_file_ops_from_message(&entry.message, &mut ops);
    }
    ops
}

fn extract_file_ops_from_message(message: &AgentMessage, ops: &mut FileOps) {
    let AgentMessage::Llm(Message::Assistant { content, .. }) = message else {
        return;
    };
    for block in content {
        let crate::types::Content::ToolCall {
            name, arguments, ..
        } = block
        else {
            continue;
        };
        let path = arguments
            .get("path")
            .or_else(|| arguments.get("file_path"))
            .or_else(|| arguments.get("filename"))
            .and_then(|v| v.as_str());
        let Some(path) = path else { continue };
        match name.as_str() {
            "edit" | "write" | "bash" => {
                ops.edited.insert(path.to_string());
            }
            "read" | "grep" | "glob" => {
                ops.read.insert(path.to_string());
            }
            _ => {}
        }
    }
}
