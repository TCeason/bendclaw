//! Sanitize assistant text before it enters context.
//!
//! Models occasionally mimic the shape of system-added reminders they see
//! wrapping prior user/tool messages — emitting `<system-reminder>…</system-reminder>`,
//! `<system>…</system>`, or status-template preambles like `Continue: …` at the
//! start of their reply. Left untouched, this feedback loop gets worse: next
//! turn sees the model's own fake tags and treats them as more teacher
//! examples to copy.
//!
//! Strategy (mirrors claudecode's `stripSystemReminders` + `stripDisplayTags`):
//!
//! 1. Strip whole `<system-reminder>…</system-reminder>` and `<system>…</system>`
//!    blocks anywhere in the text. Only lowercase tag names — uppercase/JSX
//!    (`<Button>`, `<!DOCTYPE>`) is user-authored prose and must pass through.
//! 2. If the resulting text starts with a known status-template preamble
//!    (`Continue:`, `继续：`, `Next step:`, `Status:` on its own line), drop
//!    that leading phrase. The model's real reply follows.
//!
//! Idempotent. Pure function of the input string.

/// Strip system-reminder wrapper blocks and status-template preambles from
/// assistant text. Returns the cleaned string.
pub fn sanitize_assistant_text(input: &str) -> String {
    let stripped = strip_reminder_blocks(input);
    strip_status_preamble(&stripped)
}

/// Remove `<system-reminder>…</system-reminder>` and `<system>…</system>`
/// blocks from `text`. Matches lowercase tag names only.
fn strip_reminder_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    loop {
        let Some(open_start) = find_open_tag(rest) else {
            out.push_str(rest);
            break;
        };
        let (tag_name, after_open) = match parse_open_tag(&rest[open_start..]) {
            Some(parts) => parts,
            None => {
                // Malformed — emit one byte and move on so we don't loop.
                let boundary = rest[open_start..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| open_start + i)
                    .unwrap_or(rest.len());
                out.push_str(&rest[..boundary]);
                rest = &rest[boundary..];
                continue;
            }
        };

        out.push_str(&rest[..open_start]);

        let close_tag = format!("</{tag_name}>");
        let after_open_idx = open_start + (rest[open_start..].len() - after_open.len());
        let search_region = &rest[after_open_idx..];
        match search_region.find(&close_tag) {
            Some(rel_close) => {
                let close_end = after_open_idx + rel_close + close_tag.len();
                // Swallow one trailing newline so stripping doesn't leave a blank line.
                let trim_end = rest[close_end..]
                    .strip_prefix('\n')
                    .map(|_| close_end + 1)
                    .unwrap_or(close_end);
                rest = &rest[trim_end..];
            }
            None => {
                // Unclosed tag — drop everything from here on. A stray
                // `<system-reminder>` with no close is almost certainly
                // hallucinated scaffolding, not legitimate prose.
                break;
            }
        }
    }

    out
}

/// Return the byte offset of the next `<system>` or `<system-reminder>` open
/// tag, or `None` if neither appears.
fn find_open_tag(text: &str) -> Option<usize> {
    const CANDIDATES: [&str; 2] = ["<system-reminder", "<system"];
    let mut earliest: Option<usize> = None;
    for needle in CANDIDATES {
        if let Some(idx) = text.find(needle) {
            earliest = Some(earliest.map_or(idx, |prev| prev.min(idx)));
        }
    }
    earliest
}

/// Parse an open tag starting at `slice[0]`. Returns `(tag_name, rest_after_>)`
/// when valid. Rejects anything that doesn't cleanly close with `>`.
fn parse_open_tag(slice: &str) -> Option<(&'static str, &str)> {
    // Try the longer tag first so "<system-reminder" doesn't match as "<system".
    for name in ["system-reminder", "system"] {
        let open = format!("<{name}");
        if let Some(rest) = slice.strip_prefix(&open) {
            // Next char must be `>` or whitespace (no attributes today, but be lenient).
            let rest = rest.trim_start_matches(|c: char| c != '>');
            let after_close = rest.strip_prefix('>')?;
            return Some((static_tag_name(name), after_close));
        }
    }
    None
}

fn static_tag_name(name: &str) -> &'static str {
    match name {
        "system-reminder" => "system-reminder",
        "system" => "system",
        _ => "system",
    }
}

/// Drop a leading status-template preamble produced by mimicking reminders.
/// Only matches at the very start of the text (after leading whitespace).
fn strip_status_preamble(text: &str) -> String {
    const PREFIXES: &[&str] = &[
        "Continue:",
        "Continue：",
        "继续：",
        "继续:",
        "Next step:",
        "Next:",
        "Status:",
    ];

    let leading_ws_end = text
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let head = &text[leading_ws_end..];

    for prefix in PREFIXES {
        if let Some(body) = head.strip_prefix(prefix) {
            // Keep the body minus the stripped prefix, preserve trailing text.
            let mut result = String::with_capacity(text.len() - prefix.len());
            result.push_str(body.trim_start_matches([' ', '\t']));
            return result;
        }
    }

    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_system_reminder_block() {
        let input = "<system-reminder>\nbe careful\n</system-reminder>\nActual reply.";
        assert_eq!(sanitize_assistant_text(input), "Actual reply.");
    }

    #[test]
    fn strips_bare_system_block() {
        let input = "<system>继续：目标是 X</system>\n具体步骤如下。";
        assert_eq!(sanitize_assistant_text(input), "具体步骤如下。");
    }

    #[test]
    fn strips_continue_preamble() {
        let input = "Continue: update markdown.ts and run tests.";
        assert_eq!(
            sanitize_assistant_text(input),
            "update markdown.ts and run tests."
        );
    }

    #[test]
    fn strips_chinese_preamble() {
        let input = "继续：改浅色主题。";
        assert_eq!(sanitize_assistant_text(input), "改浅色主题。");
    }

    #[test]
    fn preserves_user_prose_with_uppercase_tags() {
        let input = "Use a <Button> component and check <!DOCTYPE html>.";
        assert_eq!(sanitize_assistant_text(input), input);
    }

    #[test]
    fn preserves_inline_less_than() {
        let input = "when x < y the loop exits";
        assert_eq!(sanitize_assistant_text(input), input);
    }

    #[test]
    fn drops_unclosed_reminder_tail() {
        let input = "prelude\n<system-reminder>\nno close here";
        assert_eq!(sanitize_assistant_text(input), "prelude\n");
    }

    #[test]
    fn idempotent() {
        let input = "<system>ignore</system>\nContinue: go";
        let once = sanitize_assistant_text(input);
        assert_eq!(sanitize_assistant_text(&once), once);
        assert_eq!(once, "go");
    }
}
