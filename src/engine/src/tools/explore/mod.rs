//! Read-only code exploration tools: `grep` (content search) and `glob`
//! (file-name search).
//!
//! Both run in-process on the `ignore` + `globset` + `regex` crates — the same
//! libraries ripgrep and fd are built on — so traversal is gitignore-aware
//! without spawning an external binary or parsing its output.
//!
//! Results are returned as structured text the model can act on directly:
//! grep emits `path:line: text`, glob emits sorted relative paths. This is the
//! key difference from shelling out to `bash grep` — the agent trusts and
//! reuses the line-numbered output instead of re-reading files to locate code.

mod glob;
mod grep;

pub use glob::GlobTool;
pub use grep::GrepTool;

/// Max bytes of tool output before truncation (matches read/web_fetch caps).
pub(crate) const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Cap a result string to `MAX_OUTPUT_BYTES`, appending a truncation note.
/// Splits on a char boundary so multibyte content is never cut mid-character.
pub(crate) fn cap_output(mut s: String, note: &str) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s;
    }
    let mut end = MAX_OUTPUT_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str(&format!(
        "\n... (truncated at {} chars{note})",
        MAX_OUTPUT_BYTES
    ));
    s
}
