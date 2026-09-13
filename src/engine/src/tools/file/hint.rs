//! Locate the region of a file that most resembles an `oldText` that failed
//! to match, so the model can see exactly which lines differ.
//!
//! Pure functions — no IO.

use super::snippet::LineRange;
use super::snippet::{self};

/// Closest region of `content` to `old_text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closest {
    /// Lines of `content` aligned with `old_text`.
    pub range: LineRange,
    /// 1-based line numbers within `range` whose trimmed text differs from
    /// the corresponding `old_text` line.
    pub differing: Vec<usize>,
    /// Number of `old_text` lines that matched.
    pub matched: usize,
}

/// Slide a window the size of `old_text` over `content` and pick the window
/// with the most trimmed-line matches. Returns `None` when nothing is
/// recognisably similar (fewer than half the lines match and no
/// distinctive line is found).
pub fn closest(content: &str, old_text: &str) -> Option<Closest> {
    let old: Vec<&str> = old_text.lines().map(str::trim).collect();
    let lines: Vec<&str> = content.lines().map(str::trim).collect();
    let k = old.len();
    if k == 0 || lines.len() < k {
        return None;
    }

    let mut best: Option<(usize, usize)> = None; // (matched, start_idx)
    for i in 0..=lines.len() - k {
        let matched = old
            .iter()
            .zip(&lines[i..i + k])
            .filter(|(a, b)| a == b)
            .count();
        if best.is_none_or(|(m, _)| matched > m) {
            best = Some((matched, i));
            if matched == k {
                break;
            }
        }
    }
    let (matched, start) = best?;

    if matched * 2 < k {
        // Too weak: fall back to the longest distinctive line, if any.
        let anchor = old
            .iter()
            .enumerate()
            .filter(|(_, l)| l.len() >= 8)
            .max_by_key(|(_, l)| l.len())?;
        let hit = lines.iter().position(|l| l.contains(anchor.1))?;
        let start = hit.saturating_sub(anchor.0);
        return Some(build(&old, &lines, start));
    }
    Some(build(&old, &lines, start))
}

fn build(old: &[&str], lines: &[&str], start: usize) -> Closest {
    let end = (start + old.len()).min(lines.len());
    let differing: Vec<usize> = (start..end)
        .filter(|&i| old[i - start] != lines[i])
        .map(|i| i + 1)
        .collect();
    Closest {
        range: LineRange {
            start: start + 1,
            end,
        },
        matched: (end - start) - differing.len(),
        differing,
    }
}

/// Render `closest` as a model-facing hint, or `None` if nothing similar.
pub fn not_found_hint(content: &str, old_text: &str) -> Option<String> {
    let c = closest(content, old_text)?;
    let total = snippet::line_count(content);
    let shown = c.range.widen(2, total);
    let old_lines = old_text.lines().count();
    let mut out = format!(
        "Closest match at lines {}-{} ({} of {} lines identical; '!' marks lines that differ from your oldText):\n",
        c.range.start, c.range.end, c.matched, old_lines
    );
    out.push_str(&snippet::render(content, shown, &c.differing, 60));
    Some(out)
}
