//! Git output filtering: diff / show / log / status.
//!
//! Strategy:
//! - diff/show: keep hunk headers and +/- lines; fold long unchanged context;
//!   cap hunks per file; binary files stay as-is.
//! - log without `-p`: one compact line per commit.
//! - status: preserve section headers, fold long lists.

use super::super::filter::CmdCtx;
use super::super::filter::CmdFilter;
use super::super::filter::Stream;

const CONTEXT_LINES: usize = 3;
const MAX_HUNKS_PER_FILE: usize = 8;
const MAX_STATUS_ENTRIES: usize = 50;

pub struct GitDiffFilter;
pub struct GitLogFilter;
pub struct GitStatusFilter;

impl CmdFilter for GitDiffFilter {
    fn id(&self) -> &'static str {
        "git_diff"
    }

    fn apply(&self, _ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> Option<String> {
        if stream != Stream::Stdout {
            return None;
        }
        if !text.contains("diff --git") && !text.contains("\ndiff ") {
            return None;
        }
        Some(compress_diff(text))
    }
}

impl CmdFilter for GitLogFilter {
    fn id(&self) -> &'static str {
        "git_log"
    }

    fn apply(&self, ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> Option<String> {
        if stream != Stream::Stdout {
            return None;
        }
        // `git log -p` / `--patch` contains diffs — treat like a diff.
        if ctx.full.contains("-p")
            || ctx.full.contains("--patch")
            || ctx.full.contains("--stat")
            || text.contains("diff --git")
        {
            return Some(compress_diff(text));
        }
        Some(compress_log(text))
    }
}

impl CmdFilter for GitStatusFilter {
    fn id(&self) -> &'static str {
        "git_status"
    }

    fn apply(&self, _ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> Option<String> {
        if stream != Stream::Stdout {
            return None;
        }
        Some(compress_status(text))
    }
}

// ---------------------------------------------------------------------------
// diff compression
// ---------------------------------------------------------------------------

fn compress_diff(text: &str) -> String {
    let mut out = String::with_capacity(text.len() / 2);
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut hunks_in_file: usize = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("diff --git") {
            hunks_in_file = 0;
            out.push_str(line);
            out.push('\n');
            i += 1;
            // Keep the small metadata block that follows (`index`, `---`, `+++`,
            // `new file mode`, `Binary files ... differ`).
            while i < lines.len()
                && !lines[i].starts_with("@@")
                && !lines[i].starts_with("diff --git")
            {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }
            continue;
        }

        if line.starts_with("@@") {
            hunks_in_file += 1;
            if hunks_in_file > MAX_HUNKS_PER_FILE {
                // Skip remaining hunks in this file.
                let mut skipped = 0usize;
                while i < lines.len() && !lines[i].starts_with("diff --git") {
                    if lines[i].starts_with("@@") {
                        skipped += 1;
                    }
                    i += 1;
                }
                if skipped > 0 {
                    out.push_str(&format!("... +{skipped} more hunks in this file\n"));
                }
                continue;
            }

            out.push_str(line);
            out.push('\n');
            i += 1;

            // Walk the hunk body, folding runs of unchanged context lines.
            let mut ctx_run: Vec<&str> = Vec::new();
            while i < lines.len()
                && !lines[i].starts_with("@@")
                && !lines[i].starts_with("diff --git")
            {
                let l = lines[i];
                let is_change = l.starts_with('+') || l.starts_with('-');
                if is_change {
                    if !ctx_run.is_empty() {
                        flush_context(&mut out, &ctx_run);
                        ctx_run.clear();
                    }
                    out.push_str(l);
                    out.push('\n');
                } else {
                    ctx_run.push(l);
                }
                i += 1;
            }
            if !ctx_run.is_empty() {
                flush_context(&mut out, &ctx_run);
            }
            continue;
        }

        // Anything else: pass through.
        out.push_str(line);
        out.push('\n');
        i += 1;
    }

    out
}

fn flush_context(out: &mut String, ctx: &[&str]) {
    if ctx.len() <= CONTEXT_LINES * 2 {
        for l in ctx {
            out.push_str(l);
            out.push('\n');
        }
        return;
    }
    for l in &ctx[..CONTEXT_LINES] {
        out.push_str(l);
        out.push('\n');
    }
    let hidden = ctx.len() - CONTEXT_LINES * 2;
    out.push_str(&format!("  ... @@ {hidden} unchanged lines\n"));
    for l in &ctx[ctx.len() - CONTEXT_LINES..] {
        out.push_str(l);
        out.push('\n');
    }
}

// ---------------------------------------------------------------------------
// log compression (no patch)
// ---------------------------------------------------------------------------

fn compress_log(text: &str) -> String {
    // Best-effort parse of default `git log` output:
    //   commit <sha>
    //   Author: Name <email>
    //   Date:   ...
    //   <blank>
    //       subject
    //       ...
    let mut out = String::new();
    let mut cur_sha = String::new();
    let mut cur_author = String::new();
    let mut cur_date = String::new();
    let mut cur_subject = String::new();
    let mut in_body = false;

    let flush = |out: &mut String, sha: &str, author: &str, date: &str, subj: &str| {
        if sha.is_empty() {
            return;
        }
        let short_sha = sha.chars().take(7).collect::<String>();
        let short_date = date
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        let short_author = author
            .split('<')
            .next()
            .unwrap_or(author)
            .trim()
            .to_string();
        out.push_str(&format!(
            "{short_sha} {short_date} {short_author}: {subj}\n"
        ));
    };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("commit ") {
            flush(
                &mut out,
                &cur_sha,
                &cur_author,
                &cur_date,
                cur_subject.trim(),
            );
            cur_sha = rest.split_whitespace().next().unwrap_or("").to_string();
            cur_author.clear();
            cur_date.clear();
            cur_subject.clear();
            in_body = false;
        } else if let Some(rest) = line.strip_prefix("Author:") {
            cur_author = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("Date:") {
            cur_date = rest.trim().to_string();
        } else if line.is_empty() {
            in_body = !cur_sha.is_empty();
        } else if in_body && cur_subject.is_empty() {
            cur_subject = line.trim().to_string();
        }
    }
    flush(
        &mut out,
        &cur_sha,
        &cur_author,
        &cur_date,
        cur_subject.trim(),
    );

    if out.is_empty() {
        // Couldn't parse — fall back to original.
        text.to_string()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// status compression
// ---------------------------------------------------------------------------

fn compress_status(text: &str) -> String {
    // Known section headers in `git status` long format.
    const SECTION_PREFIXES: &[&str] = &[
        "Changes to be committed:",
        "Changes not staged for commit:",
        "Untracked files:",
        "Unmerged paths:",
    ];

    let mut out = String::with_capacity(text.len());
    let mut section_lines: Vec<String> = Vec::new();
    let mut in_section = false;

    let flush_section = |section_lines: &mut Vec<String>, out: &mut String| {
        if section_lines.len() > MAX_STATUS_ENTRIES {
            let kept = MAX_STATUS_ENTRIES;
            for l in section_lines.iter().take(kept) {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str(&format!(
                "... +{} more entries\n",
                section_lines.len() - kept
            ));
        } else {
            for l in section_lines.iter() {
                out.push_str(l);
                out.push('\n');
            }
        }
        section_lines.clear();
    };

    for line in text.lines() {
        let is_header = SECTION_PREFIXES.iter().any(|p| line.starts_with(p));
        if is_header {
            if in_section {
                flush_section(&mut section_lines, &mut out);
            }
            out.push_str(line);
            out.push('\n');
            in_section = true;
            continue;
        }

        if in_section {
            // An empty line ends the section; anything else is an entry.
            if line.trim().is_empty() {
                flush_section(&mut section_lines, &mut out);
                out.push_str(line);
                out.push('\n');
                in_section = false;
            } else {
                section_lines.push(line.to_string());
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if in_section {
        flush_section(&mut section_lines, &mut out);
    }
    out
}
