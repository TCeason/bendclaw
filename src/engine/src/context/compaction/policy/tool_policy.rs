//! Per-tool truncation strategy.

pub const ALREADY_COMPACTED_THRESHOLD: usize = 200;

const COMPACTABLE_TOOLS: &[&str] = &["Read", "Bash", "WebFetch", "Edit", "Write"];

/// Return whether a tool result should count toward microcompaction pressure.
pub fn is_compactable_tool_result(tool_name: &str, text_len: usize) -> bool {
    COMPACTABLE_TOOLS.contains(&tool_name) && text_len >= ALREADY_COMPACTED_THRESHOLD
}

/// Per-tool truncation policy used by the shrink pass.
pub struct ToolPolicy {
    /// Tokens threshold for age-based clearing of old results.
    /// `None` means this tool is never age-cleared (e.g. `Read`).
    pub age_clear_threshold: Option<usize>,
    /// Max lines when truncating an oversized result.
    pub oversize_max_lines: usize,
    /// Max lines for normal (budget-gated) truncation.
    pub normal_max_lines: usize,
    /// Whether to prefer tree-sitter outline over head-tail truncation.
    pub prefer_outline: bool,
}

/// Return the truncation policy for a given tool name.
pub fn tool_policy(tool_name: &str, max_lines: usize) -> ToolPolicy {
    let configured_max_lines = max_lines.max(1);
    let mut policy = match tool_name {
        "Read" => ToolPolicy {
            age_clear_threshold: None,
            oversize_max_lines: 30,
            normal_max_lines: 50,
            prefer_outline: true,
        },
        "WebFetch" => ToolPolicy {
            age_clear_threshold: Some(2000),
            oversize_max_lines: 20,
            normal_max_lines: 30,
            prefer_outline: false,
        },
        "Bash" => ToolPolicy {
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
    };
    policy.oversize_max_lines = policy.oversize_max_lines.min(configured_max_lines);
    policy.normal_max_lines = policy.normal_max_lines.min(configured_max_lines);
    policy
}
