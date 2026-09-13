//! Line-numbered snippets of file content for model-facing tool output.
//!
//! All functions are pure and operate on 1-based line numbers.

/// Inclusive 1-based line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

impl LineRange {
    /// Widen by `radius` lines on both sides, clamped to `[1, total]`.
    pub fn widen(self, radius: usize, total: usize) -> LineRange {
        LineRange {
            start: self.start.saturating_sub(radius).max(1),
            end: (self.end + radius).min(total.max(1)),
        }
    }
}

/// Convert a byte offset within `text` to a 1-based line number.
pub fn line_at(text: &str, byte_offset: usize) -> usize {
    text.as_bytes()[..byte_offset.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Line range covered by the byte span `[start, end)`. An empty span maps to
/// the single line containing `start`.
pub fn range_of_span(text: &str, start: usize, end: usize) -> LineRange {
    let last = if end > start { end - 1 } else { start };
    LineRange {
        start: line_at(text, start),
        end: line_at(text, last),
    }
}

/// Merge ranges that overlap or sit within `gap` lines of each other.
/// Input order does not matter; output is sorted.
pub fn merge_ranges(mut ranges: Vec<LineRange>, gap: usize) -> Vec<LineRange> {
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<LineRange> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(prev) if r.start <= prev.end + gap + 1 => prev.end = prev.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// Render `range` of `text` as numbered lines. Lines whose 1-based number is
/// in `marked` use `!` instead of `|` as the separator. Output is capped at
/// `max_lines`; overflow is summarised on a trailing line.
pub fn render(text: &str, range: LineRange, marked: &[usize], max_lines: usize) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let total = lines.len();
    let end = range.end.min(total);
    if range.start == 0 || range.start > end {
        return String::new();
    }
    let width = end.to_string().len();
    let shown_end = end.min(range.start + max_lines.max(1) - 1);
    let mut out = String::new();
    for n in range.start..=shown_end {
        let sep = if marked.contains(&n) { '!' } else { '|' };
        out.push_str(&format!("{n:>width$} {sep} {}\n", lines[n - 1]));
    }
    if shown_end < end {
        out.push_str(&format!(
            "{:>width$}   ... ({} more lines)\n",
            "",
            end - shown_end
        ));
    }
    out
}

/// Number of lines in `text` (a trailing newline does not start a new line).
pub fn line_count(text: &str) -> usize {
    match text.strip_suffix('\n') {
        Some(t) => t.split('\n').count(),
        None => text.split('\n').count(),
    }
}
