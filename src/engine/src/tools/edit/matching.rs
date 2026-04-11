//! Matching logic for edit tool — find the unique occurrence of old_text in file content.
//!
//! Uses a tiered fallback strategy:
//! 1. Exact match
//! 2. Quote-normalized match (curly quotes → straight quotes)
//! 3. Trailing-whitespace-insensitive line match
//!
//! All functions are pure — no IO, no side effects.

use super::normalize;

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
    let norm_content = normalize::normalize_quotes(content_lf);
    let norm_old = normalize::normalize_quotes(old_text_lf);

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
