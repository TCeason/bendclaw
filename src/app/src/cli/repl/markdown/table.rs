//! Table layout: width calculation, word wrap, horizontal/vertical rendering.

use unicode_width::UnicodeWidthStr;

use super::ansi::display_width;
use super::ansi::strip_ansi_codes;
use super::theme::Theme;

const MIN_COLUMN_WIDTH: usize = 3;
const MAX_ROW_LINES: usize = 4;
const SAFETY_MARGIN: usize = 4;

// ---------------------------------------------------------------------------
// Public entry points called by Renderer
// ---------------------------------------------------------------------------

/// Decide layout, build lines, and return the full rendered table string.
pub fn render_table(theme: &Theme, rows: &[Vec<String>], terminal_width: usize) -> Vec<String> {
    let Some((header, body)) = rows.split_first() else {
        return Vec::new();
    };
    let column_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if column_count == 0 {
        return Vec::new();
    }

    let header = normalize_row(header, column_count);
    let body: Vec<Vec<String>> = body
        .iter()
        .map(|r| normalize_row(r, column_count))
        .collect();

    let min_widths = collect_min_widths(&header, &body);
    let ideal_widths = collect_ideal_widths(&header, &body);
    let border_overhead = 1 + column_count * 3;
    let available = terminal_width
        .saturating_sub(border_overhead + SAFETY_MARGIN)
        .max(column_count * MIN_COLUMN_WIDTH);

    let (col_widths, hard_wrap) = fit_column_widths(&min_widths, &ideal_widths, available);

    if should_use_vertical(&header, &body, &col_widths, hard_wrap) {
        return render_vertical(&header, &body, terminal_width);
    }

    build_horizontal(theme, &header, &body, &col_widths, hard_wrap)
}

// ---------------------------------------------------------------------------
// Row normalization
// ---------------------------------------------------------------------------

fn normalize_row(row: &[String], column_count: usize) -> Vec<String> {
    let mut out = row.to_vec();
    while out.len() < column_count {
        out.push(String::new());
    }
    out
}

// ---------------------------------------------------------------------------
// Width calculation
// ---------------------------------------------------------------------------

fn collect_min_widths(header: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths = vec![MIN_COLUMN_WIDTH; header.len()];
    for (i, cell) in header.iter().enumerate() {
        widths[i] = widths[i].max(longest_word_width(cell));
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(longest_word_width(cell));
        }
    }
    widths
}

fn collect_ideal_widths(header: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths = vec![MIN_COLUMN_WIDTH; header.len()];
    for (i, cell) in header.iter().enumerate() {
        widths[i] = widths[i].max(display_width(cell));
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(cell));
        }
    }
    widths
}

fn longest_word_width(input: &str) -> usize {
    let mut max = MIN_COLUMN_WIDTH;
    for word in strip_ansi_codes(input).split_whitespace() {
        max = max.max(UnicodeWidthStr::width(word));
    }
    max
}

fn fit_column_widths(
    min_widths: &[usize],
    ideal_widths: &[usize],
    available: usize,
) -> (Vec<usize>, bool) {
    let total_min: usize = min_widths.iter().sum();
    let total_ideal: usize = ideal_widths.iter().sum();

    let mut hard_wrap = false;
    let mut widths = if total_ideal <= available {
        ideal_widths.to_vec()
    } else if total_min <= available {
        let extra_space = available - total_min;
        let overflows: Vec<usize> = ideal_widths
            .iter()
            .zip(min_widths.iter())
            .map(|(ideal, min)| ideal.saturating_sub(*min))
            .collect();
        let total_overflow: usize = overflows.iter().sum();
        min_widths
            .iter()
            .enumerate()
            .map(|(i, min)| {
                if total_overflow == 0 {
                    *min
                } else {
                    *min + overflows[i] * extra_space / total_overflow
                }
            })
            .collect()
    } else {
        hard_wrap = true;
        let scale_num = available;
        let scale_den = total_min.max(1);
        min_widths
            .iter()
            .map(|w| ((w * scale_num) / scale_den).max(MIN_COLUMN_WIDTH))
            .collect::<Vec<_>>()
    };

    let mut total: usize = widths.iter().sum();
    while total > available {
        let Some((idx, _)) = widths
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > MIN_COLUMN_WIDTH)
            .max_by_key(|(_, w)| **w)
        else {
            break;
        };
        widths[idx] = widths[idx].saturating_sub(1);
        total = total.saturating_sub(1);
        hard_wrap = true;
    }

    (widths, hard_wrap)
}

// ---------------------------------------------------------------------------
// Vertical fallback
// ---------------------------------------------------------------------------

fn should_use_vertical(
    header: &[String],
    rows: &[Vec<String>],
    col_widths: &[usize],
    hard_wrap: bool,
) -> bool {
    let mut max_lines = 1usize;
    for (i, cell) in header.iter().enumerate() {
        let w = col_widths.get(i).copied().unwrap_or(MIN_COLUMN_WIDTH);
        max_lines = max_lines.max(wrap_plain_text(cell, w, hard_wrap).len());
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            let w = col_widths.get(i).copied().unwrap_or(MIN_COLUMN_WIDTH);
            max_lines = max_lines.max(wrap_plain_text(cell, w, hard_wrap).len());
        }
    }
    max_lines > MAX_ROW_LINES
}

fn render_vertical(header: &[String], rows: &[Vec<String>], terminal_width: usize) -> Vec<String> {
    let separator = "─".repeat(terminal_width.saturating_sub(1).clamp(3, 40));
    let mut lines = Vec::new();

    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx > 0 {
            lines.push(separator.clone());
        }
        for (col_idx, cell) in row.iter().enumerate() {
            let label = header
                .get(col_idx)
                .map(|v| strip_ansi_codes(v))
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format!("Column {}", col_idx + 1));
            let value = normalize_value(cell);

            let first_width = terminal_width
                .saturating_sub(UnicodeWidthStr::width(label.as_str()) + 2)
                .max(10);
            let rest_width = terminal_width.saturating_sub(2).max(10);
            let first_pass = wrap_plain_text(&value, first_width, false);
            let first_line = first_pass.first().cloned().unwrap_or_default();
            lines.push(format!("\x1b[1m{}:\x1b[0m {}", label, first_line));

            if first_pass.len() > 1 {
                let remaining = first_pass[1..].join(" ");
                for cont in wrap_plain_text(&remaining, rest_width, false) {
                    if !cont.trim().is_empty() {
                        lines.push(format!("  {}", cont));
                    }
                }
            }
        }
    }

    if lines.is_empty() {
        for (i, cell) in header.iter().enumerate() {
            let label = strip_ansi_codes(cell);
            if label.is_empty() {
                lines.push(format!("\x1b[1mColumn {}:\x1b[0m", i + 1));
            } else {
                lines.push(format!("\x1b[1m{}:\x1b[0m", label));
            }
        }
    }

    lines
}

fn normalize_value(input: &str) -> String {
    strip_ansi_codes(input)
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Horizontal table
// ---------------------------------------------------------------------------

fn build_horizontal(
    theme: &Theme,
    header: &[String],
    rows: &[Vec<String>],
    col_widths: &[usize],
    hard_wrap: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(border_line(theme, col_widths, BorderKind::Top));
    lines.extend(row_lines(theme, header, col_widths, hard_wrap, true));
    lines.push(border_line(theme, col_widths, BorderKind::Middle));
    for (i, row) in rows.iter().enumerate() {
        lines.extend(row_lines(theme, row, col_widths, hard_wrap, false));
        if i + 1 < rows.len() {
            lines.push(border_line(theme, col_widths, BorderKind::Middle));
        }
    }
    lines.push(border_line(theme, col_widths, BorderKind::Bottom));
    lines
}

fn row_lines(
    theme: &Theme,
    cells: &[String],
    col_widths: &[usize],
    hard_wrap: bool,
    is_header: bool,
) -> Vec<String> {
    let wrapped: Vec<Vec<String>> = col_widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let val = cells.get(i).cloned().unwrap_or_default();
            wrap_plain_text(&val, *w, hard_wrap)
        })
        .collect();

    let max = wrapped.iter().map(|l| l.len()).max().unwrap_or(1);
    let border = theme.table_border.paint("│");
    let mut result = Vec::new();

    for line_idx in 0..max {
        let mut line = border.clone();
        for (col_idx, w) in col_widths.iter().enumerate() {
            let content = wrapped
                .get(col_idx)
                .and_then(|l| l.get(line_idx))
                .cloned()
                .unwrap_or_default();
            let padded = pad_to_width(&content, *w);
            let styled = if is_header {
                theme.table_header.paint(&padded)
            } else {
                padded
            };
            line.push(' ');
            line.push_str(&styled);
            line.push(' ');
            line.push_str(&border);
        }
        result.push(line);
    }

    result
}

enum BorderKind {
    Top,
    Middle,
    Bottom,
}

fn border_line(theme: &Theme, col_widths: &[usize], kind: BorderKind) -> String {
    let (left, mid, cross, right) = match kind {
        BorderKind::Top => ("┌", "─", "┬", "┐"),
        BorderKind::Middle => ("├", "─", "┼", "┤"),
        BorderKind::Bottom => ("└", "─", "┴", "┘"),
    };

    let mut line = theme.table_border.paint(left);
    for (i, w) in col_widths.iter().enumerate() {
        line.push_str(&theme.table_border.paint(&mid.repeat(w + 2)));
        if i + 1 < col_widths.len() {
            line.push_str(&theme.table_border.paint(cross));
        } else {
            line.push_str(&theme.table_border.paint(right));
        }
    }
    line
}

fn pad_to_width(input: &str, target: usize) -> String {
    let w = display_width(input);
    let pad = target.saturating_sub(w);
    format!("{}{}", input, " ".repeat(pad))
}

// ---------------------------------------------------------------------------
// Word wrap
// ---------------------------------------------------------------------------

fn wrap_plain_text(input: &str, width: usize, hard_wrap: bool) -> Vec<String> {
    if width == 0 {
        return vec![input.to_string()];
    }

    let mut lines = Vec::new();
    for raw_line in strip_ansi_codes(input).trim_end().split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        for word in raw_line.split_whitespace() {
            let ww = UnicodeWidthStr::width(word);
            if current.is_empty() {
                if ww <= width || !hard_wrap {
                    current.push_str(word);
                    current_width = ww;
                } else {
                    let mut wrapped = hard_wrap_word(word, width);
                    if let Some(last) = wrapped.pop() {
                        lines.extend(wrapped);
                        current = last;
                        current_width = UnicodeWidthStr::width(current.as_str());
                    }
                }
                continue;
            }

            if current_width + 1 + ww <= width {
                current.push(' ');
                current.push_str(word);
                current_width += 1 + ww;
                continue;
            }

            lines.push(current);
            current = String::new();
            current_width = 0;

            if ww <= width || !hard_wrap {
                current.push_str(word);
                current_width = ww;
            } else {
                let mut wrapped = hard_wrap_word(word, width);
                if let Some(last) = wrapped.pop() {
                    lines.extend(wrapped);
                    current = last;
                    current_width = UnicodeWidthStr::width(current.as_str());
                }
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn hard_wrap_word(word: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![word.to_string()];
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in word.chars() {
        let mut buf = [0u8; 4];
        let cw = UnicodeWidthStr::width(ch.encode_utf8(&mut buf));
        if current_width + cw > width && !current.is_empty() {
            out.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += cw;
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}
