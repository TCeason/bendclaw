//! Text normalization primitives for edit matching.
//!
//! All functions are pure — no IO, no side effects.

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
/// This invariant is relied upon by `matching.rs` to map char indices
/// between normalized and original content. Do not add any transformation
/// that changes character count.
pub fn normalize_quotes(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'', // ' ' → '
            '\u{201C}' | '\u{201D}' => '"',  // " " → "
            other => other,
        })
        .collect()
}
