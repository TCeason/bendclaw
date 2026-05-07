//! Compaction marker: the synthetic message inserted into the retained
//! window to signal "messages were dropped here".
//!
//! Beyond the bookkeeping (count), the marker also carries two anti-drift
//! signals for the model:
//!   1. A verbatim quote of the most recent real user turn, so the agent
//!      can re-anchor to the active task if the summary leaves the original
//!      request ambiguous.
//!   2. A continuation instruction telling the agent NOT to "re-orient"
//!      back to older tasks that appear earlier in the retained window.
//!
//! Inspired by claudecode's post-compact continuation prompt
//! (`getCompactUserSummaryMessage`), adapted to evot's phase-based
//! compaction (we have no LLM-written summary — the marker IS the summary).
//!
//! Cost discipline: the marker itself takes tokens and is inserted
//! repeatedly as compaction runs. The full anchor + instruction is only
//! attached when eviction actually freed enough room to pay for it. Tiny
//! evictions fall back to a one-line marker so marker tokens never exceed
//! dropped tokens.

use crate::context::tokens::message_tokens;
use crate::types::*;

/// Max verbatim characters from the most-recent user message kept in the
/// marker. ~200 chars ≈ 50 tokens — enough to capture intent without
/// bloating context.
const ANCHOR_MAX_CHARS: usize = 200;

/// Approximate budget (in estimated tokens) the full anchor + instruction
/// costs. Only attached when `dropped_tokens_hint` meets this threshold;
/// otherwise we fall back to the minimal one-line marker.
const FULL_MARKER_MIN_DROPPED_TOKENS: usize = 120;

/// Build a compaction marker message with anchor + continuation instruction.
///
/// `pre_drop_messages` is the full message slice BEFORE eviction ran, used
/// to locate the latest user turn for the anchor. `dropped_tokens_hint` is
/// the approximate token cost of what was dropped; used to decide whether
/// the full marker or the minimal variant fits the budget.
pub(crate) fn build_marker(
    pre_drop_messages: &[AgentMessage],
    removed: usize,
    dropped_tokens_hint: usize,
) -> AgentMessage {
    build_marker_with_note(
        pre_drop_messages,
        &format!("{} messages removed", removed),
        dropped_tokens_hint,
    )
}

/// Variant for cases that already have a custom count phrase (e.g.
/// "N messages removed to fit context window").
pub(crate) fn build_marker_with_note(
    pre_drop_messages: &[AgentMessage],
    count_note: &str,
    dropped_tokens_hint: usize,
) -> AgentMessage {
    if dropped_tokens_hint < FULL_MARKER_MIN_DROPPED_TOKENS {
        return build_minimal(count_note);
    }
    let anchor = latest_user_text_verbatim(pre_drop_messages);
    let full = text_message(&format_full_marker(count_note, anchor.as_deref()));
    // Belt-and-braces: if the full marker turns out to cost more than what
    // was dropped (small messages but many of them), degrade to minimal.
    if message_tokens(&full) > dropped_tokens_hint {
        return build_minimal(count_note);
    }
    full
}

/// Fallback marker when the pre-drop slice isn't available (sanitize's
/// "filtered everything to empty" branch).
pub(crate) fn build_fallback_marker() -> AgentMessage {
    build_minimal("messages removed")
}

fn build_minimal(count_note: &str) -> AgentMessage {
    text_message(&format!("[Context compacted: {}]", count_note))
}

fn text_message(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        timestamp: now_ms(),
    })
}

fn format_full_marker(count_note: &str, anchor: Option<&str>) -> String {
    let mut out = format!("[Context compacted: {}]", count_note);
    if let Some(text) = anchor {
        out.push_str("\n\nMost recent user request (verbatim):\n");
        out.push_str(text);
    }
    out.push_str(
        "\n\nContinue with the most recent user request. \
         Do not re-orient to older tasks that appear earlier in the retained context.",
    );
    out
}

/// Return the most recent user-typed text in `messages`, verbatim (trimmed
/// and head-truncated to `ANCHOR_MAX_CHARS`). Skips:
///   - Tool results (role is separate, but belt-and-braces).
///   - `<system-reminder>` wrappers (internal bookkeeping, not user intent).
///   - Prior compaction markers (`[Context compacted: ...`).
fn latest_user_text_verbatim(messages: &[AgentMessage]) -> Option<String> {
    for msg in messages.iter().rev() {
        let user_content = match msg {
            AgentMessage::Llm(Message::User { content, .. }) => content,
            _ => continue,
        };
        let text = match first_plain_text(user_content) {
            Some(t) => t,
            None => continue,
        };
        if is_internal_user_message(&text) {
            continue;
        }
        return Some(truncate_anchor(&text));
    }
    None
}

fn first_plain_text(content: &[Content]) -> Option<String> {
    for c in content {
        if let Content::Text { text } = c {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn is_internal_user_message(text: &str) -> bool {
    text.starts_with("<system-reminder>") || text.starts_with("[Context compacted")
}

/// Head-only truncation with ellipsis marker when the original is longer.
/// Keeps the opening of the request — that's where intent lives.
fn truncate_anchor(text: &str) -> String {
    if text.chars().count() <= ANCHOR_MAX_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(ANCHOR_MAX_CHARS).collect();
    format!("{}…", head)
}
