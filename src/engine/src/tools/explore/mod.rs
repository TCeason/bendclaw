//! Read-only code exploration tools: `grep` (content search) and `glob`
//! (file-name search).
//!
//! Both run in-process on the `ignore` + `globset` + `regex` crates — the same
//! libraries ripgrep and fd are built on — so traversal is gitignore-aware
//! without spawning an external binary or parsing its output. Tree traversal
//! is parallelized via `ignore`'s `build_parallel` (the same machinery that
//! makes ripgrep fast), so large repositories scan across all cores.
//!
//! Results are returned as structured text the model can act on directly:
//! grep emits `path:line: text`, glob emits paths sorted by recency. This is
//! the key difference from shelling out to `bash grep` — the agent trusts and
//! reuses the line-numbered output instead of re-reading files to locate code.

mod glob;
mod grep;
mod search;

use std::path::PathBuf;

pub use glob::GlobTool;
pub use grep::GrepTool;
use parking_lot::Mutex;
pub use search::SearchTool;
use tokio_util::sync::CancellationToken;

use crate::types::ToolContext;

/// Max bytes of tool output before truncation (matches read/web_fetch caps).
pub(crate) const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Walk one or more directory trees in parallel, gitignore-aware, collecting
/// whatever `visit` returns for each entry. Mirrors the engine ripgrep/fd are
/// built on: `ignore`'s `build_parallel` fans the walk across a thread pool.
///
/// `visit` runs on worker threads, so it must be `Sync`; returning `None` skips
/// an entry. Results come back in nondeterministic order — the caller sorts.
/// The walk stops promptly when `cancel` fires.
pub(crate) fn parallel_collect<T, F>(
    roots: &[PathBuf],
    gitignore: bool,
    include_hidden: bool,
    cancel: &CancellationToken,
    visit: F,
) -> Vec<T>
where
    F: Fn(&ignore::DirEntry) -> Option<T> + Sync,
    T: Send,
{
    let mut roots_iter = roots.iter();
    let Some(first) = roots_iter.next() else {
        return Vec::new();
    };
    let mut builder = ignore::WalkBuilder::new(first);
    for r in roots_iter {
        builder.add(r);
    }
    // `git_ignore`/`ignore`/`parents`/`git_exclude`/`git_global` together cover
    // every standard ignore source; toggle them as one so `gitignore: false`
    // truly surfaces ignored files (e.g. `.env`, build outputs). `hidden(true)`
    // means "skip dotfiles", so include-hidden inverts it.
    builder
        .require_git(false)
        .git_ignore(gitignore)
        .git_global(gitignore)
        .git_exclude(gitignore)
        .ignore(gitignore)
        .parents(gitignore)
        .hidden(!include_hidden);

    let out: Mutex<Vec<T>> = Mutex::new(Vec::new());
    let out_ref = &out;
    let visit_ref = &visit;
    builder.build_parallel().run(|| {
        let cancel = cancel.clone();
        Box::new(move |result| {
            if cancel.is_cancelled() {
                return ignore::WalkState::Quit;
            }
            if let Ok(entry) = result {
                if let Some(v) = visit_ref(&entry) {
                    out_ref.lock().push(v);
                }
            }
            ignore::WalkState::Continue
        })
    });
    out.into_inner()
}

/// Truncate `s` to at most `MAX_OUTPUT_BYTES`, preferring the last newline so
/// the output never ends on a half-line. Falls back to a char boundary when a
/// single line already exceeds the cap.
pub(crate) fn cap_to_line_boundary(s: &str) -> &str {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s;
    }
    let window = &s[..MAX_OUTPUT_BYTES];
    if let Some(nl) = window.rfind('\n') {
        return &s[..nl];
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Cap a result string to `MAX_OUTPUT_BYTES`, appending a truncation note.
/// Breaks on a line boundary so multibyte content and partial lines are never
/// surfaced mid-token.
pub(crate) fn cap_output(s: String, note: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s;
    }
    let mut out = cap_to_line_boundary(&s).to_string();
    out.push_str(&format!(
        "\n... (truncated at {MAX_OUTPUT_BYTES} chars{note})"
    ));
    out
}

/// Finalize tool output: when it fits, return it; when it overflows and a spill
/// store is configured, persist the full text and return a line-capped preview
/// plus a reference the model can `read`; otherwise cap with a note.
pub(crate) async fn finalize_output(
    full: String,
    note: &str,
    ctx: &ToolContext,
    spill_key: &str,
) -> String {
    if full.len() <= MAX_OUTPUT_BYTES {
        return full;
    }
    if let Some(spill) = ctx.spill.as_ref() {
        if let Ok(spill_ref) = spill.spill_text(spill_key, &full).await {
            let preview = cap_to_line_boundary(&full);
            return format!(
                "{preview}\n... (truncated at {MAX_OUTPUT_BYTES} chars; full output \
                 spilled to {} — {} bytes. Read that path for the complete result.)",
                spill_ref.path.display(),
                spill_ref.size_bytes
            );
        }
    }
    cap_output(full, note)
}
