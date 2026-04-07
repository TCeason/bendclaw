use std::fmt;

use similar::ChangeTag;
use similar::TextDiff;

struct LineNum {
    index: Option<usize>,
    width: usize,
}

impl fmt::Display for LineNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.index {
            None => write!(f, "{:width$}", "", width = self.width),
            Some(idx) => write!(f, "{:<width$}", idx + 1, width = self.width),
        }
    }
}

pub struct DiffResult {
    pub text: String,
    pub lines_added: u64,
    pub lines_removed: u64,
}

const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn format_diff(old: &str, new: &str) -> DiffResult {
    let diff = TextDiff::from_lines(old, new);
    let ops = diff.grouped_ops(3);

    if ops.is_empty() {
        return DiffResult {
            text: format!("{DIM}(no changes){RESET}\n"),
            lines_added: 0,
            lines_removed: 0,
        };
    }

    // Calculate line number width from visible changes only
    let mut max_line = 0usize;
    for group in &ops {
        for op in group {
            for change in diff.iter_changes(op) {
                if let Some(idx) = change.old_index() {
                    max_line = max_line.max(idx + 1);
                }
                if let Some(idx) = change.new_index() {
                    max_line = max_line.max(idx + 1);
                }
            }
        }
    }
    let width = if max_line == 0 {
        1
    } else {
        (max_line as f64).log10().floor() as usize + 1
    };

    let mut out = String::new();
    let mut lines_added = 0u64;
    let mut lines_removed = 0u64;

    for (idx, group) in ops.iter().enumerate() {
        if idx > 0 {
            out.push_str(&format!("{DIM}...{RESET}\n"));
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, color) = match change.tag() {
                    ChangeTag::Delete => {
                        lines_removed += 1;
                        ("-", RED)
                    }
                    ChangeTag::Insert => {
                        lines_added += 1;
                        ("+", YELLOW)
                    }
                    ChangeTag::Equal => (" ", DIM),
                };

                let old_num = LineNum {
                    index: change.old_index(),
                    width,
                };
                let new_num = LineNum {
                    index: change.new_index(),
                    width,
                };
                out.push_str(&format!(
                    "{DIM}{old_num} {new_num} |{RESET}{color}{sign}{}{RESET}",
                    change.value()
                ));
                if change.missing_newline() {
                    out.push('\n');
                }
            }
        }
    }

    DiffResult {
        text: out,
        lines_added,
        lines_removed,
    }
}

/// Extract old_content and new_content from tool details and format a diff.
/// Returns None if the details don't contain diff-able content.
pub fn diff_from_details(details: &serde_json::Value) -> Option<String> {
    let new_content = details.get("new_content")?.as_str()?;
    let old_content = details
        .get("old_content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if old_content == new_content {
        return None;
    }
    let result = format_diff(old_content, new_content);
    Some(result.text)
}
