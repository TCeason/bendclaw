//! Edit tool — surgical search/replace edits on files.
//!
//! Supports multiple disjoint edits in one call. All edits are matched against
//! the original file content, then applied in reverse position order.
//!
//! Includes the matching logic (exact / quote-normalized / whitespace-insensitive)
//! and text normalization primitives used by the tool.

use async_trait::async_trait;

use super::diff;
use super::mutex::acquire_file_lock;
use crate::types::*;

// ─── Tool ─────────────────────────────────────────────────────────────────

/// Surgical file editing via exact text search/replace.
pub struct EditFileTool {
    disallow_message: Option<String>,
}

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditFileTool {
    pub fn new() -> Self {
        Self {
            disallow_message: None,
        }
    }

    /// Mark this tool as disallowed. `execute()` will return the given message
    /// instead of performing the edit.
    pub fn disallow(mut self, message: impl Into<String>) -> Self {
        self.disallow_message = Some(message.into());
        self
    }
}

#[async_trait]
impl AgentTool for EditFileTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "Edit".into())]
    }

    fn label(&self) -> &str {
        "Edit File"
    }

    fn description(&self) -> &str {
        "Edit a single file using exact text replacement. \
         Every edits[].old_text must match a unique, non-overlapping region of the original file. \
         If two changes affect the same block or nearby lines, merge them into one edit instead of emitting overlapping edits. \
         Do not include large unchanged regions just to connect distant changes."
    }

    fn parameter_aliases(&self) -> Option<crate::tools::validation::AliasMap> {
        Some(&[("path", &["file_path", "filePath", "file"] as &[&str])])
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative or absolute)"
                },
                "edits": {
                    "type": "array",
                    "description": "One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits. If two changes touch the same block or nearby lines, merge them into one edit instead.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": {
                                "type": "string",
                                "description": "Exact text for one targeted replacement. It must be unique in the original file and must not overlap with any other edits[].old_text in the same call."
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Replacement text for this targeted edit."
                            }
                        },
                        "required": ["old_text", "new_text"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        let n = params["edits"].as_array().map(|a| a.len()).unwrap_or(1);
        Some(format!("edit {path} ({n} replacement(s))"))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(msg) = &self.disallow_message {
            return Err(ToolError::Failed(format!("Error: {msg}")));
        }

        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path' parameter".into()))?;
        let path = ctx.path_guard.resolve_path(&ctx.cwd, path_str)?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Acquire per-file lock to serialize mutations on the same file
        let _lock = acquire_file_lock(&path).await;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Read file bytes and validate UTF-8
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            ToolError::Failed(format!(
                "Cannot read {path_str}: {e}. Use Write to create new files."
            ))
        })?;
        let raw = String::from_utf8(bytes).map_err(|_| {
            ToolError::Failed(format!(
                "Cannot edit {path_str}: only UTF-8 text files are supported."
            ))
        })?;

        // Strip BOM, detect line endings, normalize to LF
        let (bom, content_raw) = strip_utf8_bom(&raw);
        let line_ending = detect_line_ending(content_raw);
        let content_lf = normalize_to_lf(content_raw);

        // Parse edits
        let edits = self.parse_edits(&params)?;

        // Resolve all matches against original content
        let mut resolved: Vec<(usize, usize, &str, String)> = Vec::with_capacity(edits.len());
        for (i, (old_text_lf, new_text_lf)) in edits.iter().enumerate() {
            let rm = resolve_unique_match(&content_lf, old_text_lf)
                .map_err(|e| self.match_error(e, path_str, old_text_lf, &content_lf, i))?;
            let start = content_lf.find(&rm.actual_old_text).unwrap_or(0);
            let end = start + rm.actual_old_text.len();
            resolved.push((
                start,
                end,
                new_text_lf.as_str(),
                rm.kind.as_str().to_string(),
            ));
        }

        // Check for overlaps
        resolved.sort_by_key(|(start, _, _, _)| *start);
        for w in resolved.windows(2) {
            if w[0].1 > w[1].0 {
                return Err(ToolError::Failed(
                    "Edits overlap. Merge nearby changes into one edit.".into(),
                ));
            }
        }

        // Apply in reverse order to preserve offsets
        let mut new_content_lf = content_lf.clone();
        let match_kind = resolved
            .last()
            .map(|(_, _, _, k)| k.clone())
            .unwrap_or_else(|| "exact".to_string());
        let replacement_count = resolved.len();
        for (start, end, new_text, _) in resolved.into_iter().rev() {
            new_content_lf.replace_range(start..end, new_text);
        }

        // No-change detection
        if new_content_lf == content_lf {
            return Err(ToolError::Failed(format!(
                "No changes made to {path_str}. The replacement produced identical content."
            )));
        }

        // Generate diff (for details only, not sent to LLM)
        let diff_result = diff::unified_diff(&content_lf, &new_content_lf, path_str);

        // Restore BOM + original line endings and write back
        let final_content = format!(
            "{}{}",
            bom,
            restore_line_endings(&new_content_lf, line_ending)
        );
        tokio::fs::write(&path, &final_content)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot write {path_str}: {e}")))?;

        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("Updated {path_str}."),
            }],
            details: serde_json::json!({
                "path": path_str,
                "match_kind": match_kind,
                "replacement_count": replacement_count,
                "diff": diff_result.unified,
                "first_changed_line": diff_result.first_changed_line,
                "added_lines": diff_result.added_lines,
                "removed_lines": diff_result.removed_lines,
            }),
            retention: Retention::Normal,
        })
    }
}

impl EditFileTool {
    /// Parse edits from params.
    fn parse_edits(&self, params: &serde_json::Value) -> Result<Vec<(String, String)>, ToolError> {
        let arr = params["edits"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'edits' parameter".into()))?;
        if arr.is_empty() {
            return Err(ToolError::InvalidArgs("edits[] must not be empty".into()));
        }
        let mut edits = Vec::with_capacity(arr.len());
        for (i, entry) in arr.iter().enumerate() {
            let old = entry["old_text"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs(format!("edits[{i}] missing old_text")))?;
            let new = entry["new_text"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs(format!("edits[{i}] missing new_text")))?;
            let old_lf = normalize_to_lf(old);
            let new_lf = normalize_to_lf(new);
            if old_lf.is_empty() {
                return Err(ToolError::Failed(format!(
                    "edits[{i}].old_text must not be empty."
                )));
            }
            edits.push((old_lf, new_lf));
        }
        Ok(edits)
    }

    /// Format a match error with helpful hints.
    fn match_error(
        &self,
        e: MatchError,
        path_str: &str,
        old_text_lf: &str,
        content_lf: &str,
        idx: usize,
    ) -> ToolError {
        let prefix = if idx > 0 {
            format!("edits[{idx}]: ")
        } else {
            String::new()
        };
        match e {
            MatchError::EmptyOldText => {
                ToolError::Failed(format!("{prefix}old_text must not be empty."))
            }
            MatchError::NotFound => {
                let hint = find_similar_text(content_lf, old_text_lf);
                let suffix = match hint {
                    Some(similar) => format!(
                        "\n\nDid you mean:\n```\n{similar}\n```\n\
                         Make sure old_text matches the current file content exactly."
                    ),
                    None => "\n\nTip: Use Read to see the current file contents.".into(),
                };
                ToolError::Failed(format!("{prefix}old_text not found in {path_str}.{suffix}"))
            }
            MatchError::NotUnique { count } => ToolError::Failed(format!(
                "{prefix}old_text matches {count} locations in {path_str}. \
                 Include more surrounding context to make the match unique."
            )),
        }
    }
}

// ─── Matching ─────────────────────────────────────────────────────────────
//
// Tiered fallback strategy:
// 1. Exact match
// 2. Quote-normalized match (curly quotes → straight quotes)
// 3. Trailing-whitespace-insensitive line match
//
// All functions are pure — no IO, no side effects.

/// How the match was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    QuoteNormalized,
    WhitespaceInsensitive,
}

impl MatchKind {
    /// Stable string representation for serialization/details.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::QuoteNormalized => "quote_normalized",
            Self::WhitespaceInsensitive => "whitespace_insensitive",
        }
    }
}

/// A successfully resolved match — contains the actual text from the original
/// file content that can be used directly with `replacen`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMatch {
    pub actual_old_text: String,
    pub kind: MatchKind,
}

/// Matching errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchError {
    EmptyOldText,
    NotFound,
    NotUnique { count: usize },
}

/// Resolve a unique match of `old_text_lf` within `content_lf`.
///
/// Both inputs must already be LF-normalized.
/// Returns the actual text slice from `content_lf` that should be replaced.
pub fn resolve_unique_match(
    content_lf: &str,
    old_text_lf: &str,
) -> Result<ResolvedMatch, MatchError> {
    if old_text_lf.is_empty() {
        return Err(MatchError::EmptyOldText);
    }

    // Level 1: Exact match
    let count = content_lf.matches(old_text_lf).count();
    if count == 1 {
        return Ok(ResolvedMatch {
            actual_old_text: old_text_lf.to_string(),
            kind: MatchKind::Exact,
        });
    }
    if count > 1 {
        return Err(MatchError::NotUnique { count });
    }

    // Level 2: Quote-normalized match
    if let Some(result) = try_quote_normalized(content_lf, old_text_lf) {
        return result;
    }

    // Level 3: Trailing-whitespace-insensitive line match
    if let Some(result) = try_whitespace_insensitive(content_lf, old_text_lf) {
        return result;
    }

    Err(MatchError::NotFound)
}

/// Try matching after normalizing curly quotes to straight quotes.
///
/// Because `normalize_quotes` is a 1:1 char mapping, char indices in the
/// normalized string correspond exactly to char indices in the original.
fn try_quote_normalized(
    content_lf: &str,
    old_text_lf: &str,
) -> Option<Result<ResolvedMatch, MatchError>> {
    let norm_content = normalize_quotes(content_lf);
    let norm_old = normalize_quotes(old_text_lf);

    // If normalization didn't change anything, skip (already tried exact)
    if norm_content == content_lf && norm_old == old_text_lf {
        return None;
    }

    let norm_old_chars: Vec<char> = norm_old.chars().collect();
    let norm_content_chars: Vec<char> = norm_content.chars().collect();
    let search_len = norm_old_chars.len();

    if search_len == 0 || norm_content_chars.len() < search_len {
        return None;
    }

    let mut matches: Vec<usize> = Vec::new();
    for i in 0..=norm_content_chars.len() - search_len {
        if norm_content_chars[i..i + search_len] == norm_old_chars[..] {
            matches.push(i);
        }
    }

    match matches.len() {
        0 => None,
        1 => {
            // Map char index back to original content
            let start_char = matches[0];
            let actual: String = content_lf
                .chars()
                .skip(start_char)
                .take(search_len)
                .collect();
            Some(Ok(ResolvedMatch {
                actual_old_text: actual,
                kind: MatchKind::QuoteNormalized,
            }))
        }
        count => Some(Err(MatchError::NotUnique { count })),
    }
}

/// A line span: byte range `[start, end)` within the source string.
/// `end` points past the `\n` if one exists, or to the string end for the last line.
struct LineSpan {
    start: usize,
    /// Byte offset one past the end of this line (including its `\n`, if any).
    end: usize,
    /// Byte offset of the end of the line content (excluding `\n`).
    content_end: usize,
}

/// Build a line span table for `text`. Each entry records the byte range of one line.
fn build_line_spans(text: &str) -> Vec<LineSpan> {
    let mut spans = Vec::new();
    let mut pos = 0;
    for line in text.split('\n') {
        let content_end = pos + line.len();
        // end includes the '\n' if present, otherwise stops at string end
        let end = if content_end < text.len() {
            content_end + 1
        } else {
            content_end
        };
        spans.push(LineSpan {
            start: pos,
            end,
            content_end,
        });
        pos = end;
    }
    spans
}

/// Try matching lines with trailing whitespace stripped on both sides.
fn try_whitespace_insensitive(
    content_lf: &str,
    old_text_lf: &str,
) -> Option<Result<ResolvedMatch, MatchError>> {
    let old_lines: Vec<&str> = old_text_lf.lines().collect();
    let content_spans = build_line_spans(content_lf);

    if old_lines.is_empty() || content_spans.len() < old_lines.len() {
        return None;
    }

    let mut matches: Vec<usize> = Vec::new();
    for i in 0..=content_spans.len() - old_lines.len() {
        let all_match = old_lines.iter().enumerate().all(|(j, old_line)| {
            let span = &content_spans[i + j];
            content_lf[span.start..span.content_end].trim_end() == old_line.trim_end()
        });
        if all_match {
            matches.push(i);
        }
    }

    match matches.len() {
        0 => None,
        1 => {
            let start = matches[0];
            let last = start + old_lines.len() - 1;
            let byte_start = content_spans[start].start;
            // Use content_end of the last matched line (excludes trailing \n),
            // then include the \n only if old_text itself ended with one.
            let byte_end = if old_text_lf.ends_with('\n') {
                content_spans[last].end
            } else {
                content_spans[last].content_end
            };
            let actual = content_lf[byte_start..byte_end].to_string();
            Some(Ok(ResolvedMatch {
                actual_old_text: actual,
                kind: MatchKind::WhitespaceInsensitive,
            }))
        }
        count => Some(Err(MatchError::NotUnique { count })),
    }
}

/// Try to find similar text in the file content for error hints.
///
/// Looks for lines containing the first line of `target` and returns
/// a snippet of surrounding context.
pub fn find_similar_text(content: &str, target: &str) -> Option<String> {
    let target_trimmed = target.trim();
    if target_trimmed.is_empty() {
        return None;
    }

    let first_line = target_trimmed.lines().next()?;
    let first_line_trimmed = first_line.trim();

    if first_line_trimmed.is_empty() {
        return None;
    }

    let lines: Vec<&str> = content.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains(first_line_trimmed) {
            let start = i;
            let target_line_count = target_trimmed.lines().count();
            let end = (i + target_line_count + 1).min(lines.len());
            return Some(lines[start..end].join("\n"));
        }
    }

    None
}

// ─── Normalization ───────────────────────────────────────────────────────
//
// Text normalization primitives for edit matching.
// All functions are pure — no IO, no side effects.

/// Line ending style detected in file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

/// Detect the dominant line ending in `content` by counting occurrences.
pub fn detect_line_ending(content: &str) -> LineEnding {
    let crlf = content.matches("\r\n").count();
    // Total LF minus those that are part of CRLF = standalone LF count
    let lf = content.matches('\n').count().saturating_sub(crlf);
    if crlf > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

/// Normalize all line endings to LF.
pub fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Restore line endings from LF to the given style.
pub fn restore_line_endings(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => text.to_string(),
        LineEnding::CrLf => text.replace('\n', "\r\n"),
    }
}

/// Strip UTF-8 BOM if present.
/// Returns `(bom_str, content_without_bom)` where `bom_str` is `"\u{FEFF}"` or `""`.
pub fn strip_utf8_bom(content: &str) -> (&str, &str) {
    if let Some(stripped) = content.strip_prefix('\u{FEFF}') {
        ("\u{FEFF}", stripped)
    } else {
        ("", content)
    }
}

/// Normalize curly/smart quotes to ASCII straight quotes.
///
/// IMPORTANT: This is a length-preserving, 1:1 Unicode character replacement.
/// Each curly quote maps to exactly one straight quote character.
/// This invariant is relied upon by the matching code to map char indices
/// between normalized and original content. Do not add any transformation
/// that changes character count.
pub fn normalize_quotes(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'', // ‘ ’ → '
            '\u{201C}' | '\u{201D}' => '"',  // “ ” → "
            other => other,
        })
        .collect()
}

/// Preserve the curly-quote style from the file when the match was
/// quote-normalized.
///
/// When `actual_old_text` (from the file) contains curly quotes but
/// `old_text` (from the agent) used straight quotes, we apply the same
/// curly style to `new_text` so the replacement doesn't break the file's
/// typography.
///
/// If no quote normalization happened (`old_text == actual_old_text`),
/// returns `new_text` unchanged.
pub fn preserve_quote_style(old_text: &str, actual_old_text: &str, new_text: &str) -> String {
    if old_text == actual_old_text {
        return new_text.to_string();
    }

    let has_double = actual_old_text.contains('\u{201C}') || actual_old_text.contains('\u{201D}');
    let has_single = actual_old_text.contains('\u{2018}') || actual_old_text.contains('\u{2019}');

    if !has_double && !has_single {
        return new_text.to_string();
    }

    let mut result = new_text.to_string();
    if has_double {
        result = apply_curly_double_quotes(&result);
    }
    if has_single {
        result = apply_curly_single_quotes(&result);
    }
    result
}

/// Returns `true` if the character before `index` is whitespace, start of
/// string, or an opening bracket — i.e. the quote at `index` is an opening
/// quote.
fn is_opening_context(chars: &[char], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    matches!(
        chars[index - 1],
        ' ' | '\t' | '\n' | '\r' | '(' | '[' | '{' | '\u{2014}' | '\u{2013}'
    )
}

fn apply_curly_double_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' {
            if is_opening_context(&chars, i) {
                out.push('\u{201C}'); // “
            } else {
                out.push('\u{201D}'); // ”
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn apply_curly_single_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            // Apostrophe in a contraction (letter'letter) → right single curly
            let prev_is_letter = i > 0 && chars[i - 1].is_alphabetic();
            let next_is_letter = i + 1 < chars.len() && chars[i + 1].is_alphabetic();
            if prev_is_letter && next_is_letter {
                out.push('\u{2019}'); // ’
            } else if is_opening_context(&chars, i) {
                out.push('\u{2018}'); // ‘
            } else {
                out.push('\u{2019}'); // ’
            }
        } else {
            out.push(c);
        }
    }
    out
}
