//! Outline-based truncation for code files.

use std::collections::HashMap;

use crate::types::*;

/// Try to produce a tree-sitter-style outline of the content.
/// Returns None if not applicable or content is not code-like.
pub fn try_outline(
    content: &[Content],
    _tool_name: &str,
    tool_call_id: &str,
    tool_call_index: &HashMap<String, String>,
) -> Option<Vec<Content>> {
    // Get the file path from the tool_use input
    let input = tool_call_index.get(tool_call_id)?;
    let path = extract_path(input)?;

    // Only attempt outline for known code extensions
    if !is_code_file(&path) {
        return None;
    }

    let text = content.iter().find_map(|c| match c {
        Content::Text { text } => Some(text.as_str()),
        _ => None,
    })?;

    let outline = build_outline(text)?;
    if outline.is_empty() {
        return None;
    }

    Some(vec![Content::Text {
        text: format!("[Structural outline of {path}]\n{outline}"),
    }])
}

fn extract_path(input: &str) -> Option<String> {
    // Simple JSON field extraction for "path"
    let pattern = "\"path\"";
    let pos = input.find(pattern)?;
    let after = &input[pos + pattern.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let after = after.trim_start().strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

fn is_code_file(path: &str) -> bool {
    let extensions = [
        ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".rb", ".swift", ".kt",
    ];
    extensions.iter().any(|ext| path.ends_with(ext))
}

/// Build a simple outline from indentation-based structure detection.
fn build_outline(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 20 {
        return None; // Too short to need an outline
    }

    let mut outline_lines = Vec::new();
    for line in &lines {
        let trimmed = strip_tool_line_prefix(line).trim();
        if is_definition_line(trimmed) {
            outline_lines.push(trimmed.to_string());
        }
    }

    if outline_lines.is_empty() {
        return None;
    }

    Some(outline_lines.join("\n"))
}

fn strip_tool_line_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some((prefix, rest)) = trimmed.split_once('|') {
        if prefix.trim().chars().all(|c| c.is_ascii_digit()) {
            return rest;
        }
    }
    line
}

fn is_definition_line(line: &str) -> bool {
    let markers = [
        "fn ",
        "pub fn ",
        "pub(crate) fn ",
        "async fn ",
        "pub async fn ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "trait ",
        "pub trait ",
        "impl ",
        "mod ",
        "pub mod ",
        "pub(crate) mod ",
        "def ",
        "class ",
        "function ",
        "export ",
        "interface ",
        "func ",
        "type ",
        "const ",
        "pub const ",
    ];
    markers.iter().any(|m| line.starts_with(m))
}
