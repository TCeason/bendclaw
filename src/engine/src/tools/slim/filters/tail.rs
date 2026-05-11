//! Generic head+tail truncation — the fallback when no specialized filter applies.

use super::super::filter::CmdCtx;
use super::super::filter::CmdFilter;
use super::super::filter::Stream;

const MAX_BYTES: usize = 8 * 1024;
const MAX_LINES: usize = 300;
const HEAD_BYTES: usize = 2 * 1024;
const TAIL_BYTES: usize = 4 * 1024;
const HEAD_LINES: usize = 120;
const TAIL_LINES: usize = 120;

pub struct TailFilter;

impl CmdFilter for TailFilter {
    fn id(&self) -> &'static str {
        "tail"
    }

    fn apply(&self, _ctx: &CmdCtx<'_>, _stream: Stream, text: &str) -> Option<String> {
        let line_count = text.lines().count();
        if text.len() <= MAX_BYTES && line_count <= MAX_LINES {
            return None;
        }

        if text.len() <= MAX_BYTES {
            return compress_by_lines(text, line_count);
        }

        let head_end = floor_boundary(text, HEAD_BYTES);
        let tail_start = ceil_boundary(text, text.len().saturating_sub(TAIL_BYTES));
        if head_end >= tail_start {
            return None;
        }

        let omitted_bytes = tail_start - head_end;
        let omitted_lines = text[head_end..tail_start].matches('\n').count();

        let mut out = String::with_capacity(head_end + TAIL_BYTES + 64);
        out.push_str(&text[..head_end]);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!(
            "<... slimmed {} bytes ({} lines) ...>\n",
            omitted_bytes, omitted_lines
        ));
        out.push_str(&text[tail_start..]);
        Some(out)
    }
}

fn compress_by_lines(text: &str, line_count: usize) -> Option<String> {
    if line_count <= HEAD_LINES + TAIL_LINES {
        return None;
    }

    let head_end = byte_index_after_lines(text, HEAD_LINES);
    let tail_start = byte_index_before_last_lines(text, TAIL_LINES);
    if head_end >= tail_start {
        return None;
    }

    let omitted_lines = line_count.saturating_sub(HEAD_LINES + TAIL_LINES);
    let omitted_bytes = tail_start - head_end;
    let mut out = String::with_capacity(text.len().saturating_sub(omitted_bytes) + 64);
    out.push_str(&text[..head_end]);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "<... slimmed {} bytes ({} lines) ...>\n",
        omitted_bytes, omitted_lines
    ));
    out.push_str(&text[tail_start..]);
    Some(out)
}

fn byte_index_after_lines(text: &str, lines: usize) -> usize {
    if lines == 0 {
        return 0;
    }

    let mut seen = 0;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            seen += 1;
            if seen == lines {
                return idx + ch.len_utf8();
            }
        }
    }
    text.len()
}

fn byte_index_before_last_lines(text: &str, lines: usize) -> usize {
    if lines == 0 {
        return text.len();
    }

    let mut seen = 0;
    for (idx, ch) in text.char_indices().rev() {
        if ch == '\n' {
            seen += 1;
            if seen == lines {
                return idx + ch.len_utf8();
            }
        }
    }
    0
}

fn floor_boundary(s: &str, idx: usize) -> usize {
    let clamped = idx.min(s.len());
    let mut i = clamped;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}
