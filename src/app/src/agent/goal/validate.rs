//! Condition text validation for `/goal`.

use crate::error::EvotError;
use crate::error::Result;

/// Maximum allowed condition length in characters.
pub const MAX_CONDITION_LEN: usize = 4000;

/// Sentinel substrings the condition must not contain so it cannot break
/// out of the `<goal>` / `<active_goal>` / `<condition>` envelope rendered by prompt templates.
const FORBIDDEN_SUBSTRINGS: &[&str] = &[
    "</goal>",
    "<goal>",
    "</active_goal>",
    "<active_goal>",
    "</condition>",
    "<condition>",
];

/// Validate and canonicalize a goal condition string.
///
/// - Trims surrounding whitespace.
/// - Rejects empty input.
/// - Rejects > [`MAX_CONDITION_BYTES`] bytes.
/// - Rejects ASCII control chars (0x00..=0x1F) except `\t` and `\n`.
/// - Collapses runs of `\n\n+` to a single `\n`.
/// - Rejects forbidden tag substrings.
pub fn validate_condition(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(EvotError::Agent("goal condition is empty".into()));
    }
    if trimmed.chars().count() > MAX_CONDITION_LEN {
        return Err(EvotError::Agent(format!(
            "Goal condition is limited to {} characters (got {})",
            MAX_CONDITION_LEN,
            trimmed.chars().count()
        )));
    }
    for ch in trimmed.chars() {
        if ch.is_ascii_control() && ch != '\t' && ch != '\n' {
            return Err(EvotError::Agent(format!(
                "goal condition contains control character U+{:04X}",
                ch as u32
            )));
        }
    }
    for forbidden in FORBIDDEN_SUBSTRINGS {
        if trimmed.contains(forbidden) {
            return Err(EvotError::Agent(format!(
                "goal condition contains forbidden substring '{forbidden}'"
            )));
        }
    }
    let collapsed = collapse_blank_lines(trimmed);
    Ok(collapsed)
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_was_newline = false;
    for ch in s.chars() {
        if ch == '\n' {
            if prev_was_newline {
                continue;
            }
            prev_was_newline = true;
            out.push(ch);
        } else {
            prev_was_newline = false;
            out.push(ch);
        }
    }
    out
}
