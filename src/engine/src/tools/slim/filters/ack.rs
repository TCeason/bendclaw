//! Ack-style commands: only success/failure and a brief fact matter.
//!
//! Collapses successful output of commands like `git add`, `git commit`,
//! `git push`, `git pull`, `npm install` into a single line. Failures
//! are never collapsed here — the router already routes `exit != 0`
//! through the `raw_error` path before any filter sees it.

use super::super::filter::CmdCtx;
use super::super::filter::CmdFilter;
use super::super::filter::Stream;

pub struct AckFilter;

impl CmdFilter for AckFilter {
    fn id(&self) -> &'static str {
        "ack"
    }

    fn apply(&self, ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> Option<String> {
        // Only collapse stdout; stderr (warnings, progress) stays passthrough
        // so the agent keeps any non-fatal signal.
        if stream != Stream::Stdout || text.trim().is_empty() {
            return None;
        }

        let head = ctx.head;
        let sub = ctx.subcmd().unwrap_or("");
        let joined = text.to_string();

        match (head, sub) {
            ("git", "add") => Some(summarize_git_add(&joined)),
            ("git", "commit") => Some(summarize_git_commit(&joined)),
            ("git", "push") => Some(summarize_git_push(&joined)),
            ("git", "pull") => Some(summarize_git_pull(&joined)),
            ("npm" | "pnpm" | "yarn" | "bun", _) if is_install(ctx.full) => {
                Some(summarize_pkg_install(head, &joined))
            }
            _ => None,
        }
    }
}

fn is_install(full: &str) -> bool {
    let mut it = full.split_whitespace().skip(1);
    matches!(
        it.next(),
        Some("install" | "i" | "add") | Some("ci") | Some("upgrade")
    )
}

fn summarize_git_add(_text: &str) -> String {
    // `git add` prints nothing on success, rarely status hints on failure.
    // We keep `ok: staged` as a stable terse ack for LLM consumption.
    "ok: staged".to_string()
}

fn summarize_git_commit(text: &str) -> String {
    // Typical line: "[main abc1234] subject"
    // Then: " 3 files changed, 42 insertions(+), 8 deletions(-)"
    let mut sha = String::new();
    let mut subject = String::new();
    let mut files = 0usize;
    let mut added = 0usize;
    let mut deleted = 0usize;

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('[') {
            // "main abc1234] subject"
            if let Some(end) = rest.find(']') {
                let head = &rest[..end];
                let tail = rest[end + 1..].trim();
                if let Some(space) = head.rfind(' ') {
                    sha = head[space + 1..].to_string();
                }
                subject = tail.to_string();
            }
        } else if line.contains("file") && line.contains("changed") {
            files = first_number(line).unwrap_or(0);
            added = number_before(line, "insertion").unwrap_or(0);
            deleted = number_before(line, "deletion").unwrap_or(0);
        }
    }

    if sha.is_empty() {
        return text.trim().to_string();
    }
    let mut out = format!(
        "ok: {sha} {files} file{}",
        if files == 1 { "" } else { "s" }
    );
    if added > 0 || deleted > 0 {
        out.push_str(&format!(" +{added} -{deleted}"));
    }
    if !subject.is_empty() {
        out.push_str(" · ");
        out.push_str(&subject);
    }
    out
}

fn summarize_git_push(text: &str) -> String {
    // Look for a line of the form: "   deadbee..cafebab  main -> main"
    // or "* [new branch]      feat -> feat"
    for line in text.lines() {
        let t = line.trim();
        if t.contains("->") && !t.starts_with("remote:") {
            return format!("ok: push {t}");
        }
    }
    if text.contains("Everything up-to-date") {
        return "ok: push up-to-date".to_string();
    }
    text.trim().to_string()
}

fn summarize_git_pull(text: &str) -> String {
    if text.contains("Already up to date") || text.contains("Already up-to-date") {
        return "ok: pull up-to-date".to_string();
    }
    // Look for `Fast-forward` section + summary line.
    let mut files = 0usize;
    let mut added = 0usize;
    let mut deleted = 0usize;
    let mut saw_summary = false;
    for line in text.lines() {
        let t = line.trim();
        if t.contains("file") && t.contains("changed") {
            files = first_number(t).unwrap_or(0);
            added = number_before(t, "insertion").unwrap_or(0);
            deleted = number_before(t, "deletion").unwrap_or(0);
            saw_summary = true;
        }
    }
    if saw_summary {
        format!(
            "ok: pull {files} file{} +{added} -{deleted}",
            if files == 1 { "" } else { "s" }
        )
    } else {
        text.trim().to_string()
    }
}

fn summarize_pkg_install(head: &str, text: &str) -> String {
    // Try to pick up common "added N packages" lines.
    for line in text.lines().rev() {
        let t = line.trim();
        if (t.contains("added") || t.contains("installed")) && t.contains("package") {
            return format!("ok: {head} {t}");
        }
    }
    format!("ok: {head} install")
}

fn first_number(s: &str) -> Option<usize> {
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

fn number_before(s: &str, keyword: &str) -> Option<usize> {
    let idx = s.find(keyword)?;
    let head = &s[..idx];
    let mut digits = String::new();
    for c in head.chars().rev() {
        if c.is_ascii_digit() {
            digits.insert(0, c);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}
