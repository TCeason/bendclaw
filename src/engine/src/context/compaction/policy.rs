//! Compaction strategy definitions.
//!
//! All tunable parameters live here. Passes read from this module
//! to decide *what* to do — they never hard-code tool names or thresholds.

/// Per-tool truncation strategy.
pub struct ToolPolicy {
    /// Tokens threshold for age-based clearing of old results.
    /// `None` means this tool is never age-cleared (e.g. `read_file`).
    pub age_clear_threshold: Option<usize>,
    /// Max lines when truncating an oversized result.
    pub oversize_max_lines: usize,
    /// Max lines for normal (budget-gated) truncation.
    pub normal_max_lines: usize,
    /// Whether to prefer tree-sitter outline over head-tail truncation.
    pub prefer_outline: bool,
}

/// Global compaction thresholds.
///
/// These control *when* a tool result is considered oversized.
/// Per-tool `ToolPolicy` controls *how* it is handled once identified.
pub struct CompactionPolicy {
    /// Absolute token threshold — a single result above this is oversized.
    pub oversize_abs_tokens: usize,
    /// Ratio threshold — a single result above `budget * ratio` is oversized.
    pub oversize_budget_ratio: f64,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            oversize_abs_tokens: 6000,
            oversize_budget_ratio: 0.20,
        }
    }
}

/// Return the truncation policy for a given tool name.
pub fn tool_policy(tool_name: &str) -> ToolPolicy {
    match tool_name {
        "read_file" => ToolPolicy {
            age_clear_threshold: None,
            oversize_max_lines: 30,
            normal_max_lines: 50,
            prefer_outline: true,
        },
        "web_fetch" => ToolPolicy {
            age_clear_threshold: Some(2000),
            oversize_max_lines: 20,
            normal_max_lines: 30,
            prefer_outline: false,
        },
        "bash" | "search" | "list_files" => ToolPolicy {
            age_clear_threshold: Some(4000),
            oversize_max_lines: 25,
            normal_max_lines: 40,
            prefer_outline: false,
        },
        _ => ToolPolicy {
            age_clear_threshold: Some(4000),
            oversize_max_lines: 30,
            normal_max_lines: 50,
            prefer_outline: false,
        },
    }
}
