//! Edit tool — surgical search/replace edits on files.
//!
//! Supports multiple disjoint edits in one call. All edits are matched against
//! the original file content, then applied in reverse position order.

use async_trait::async_trait;

use super::diff;
use super::matching;
use super::matching::MatchError;
use super::normalize;
use crate::types::*;

/// Surgical file editing via exact text search/replace.
pub struct EditFileTool {
    disallow_message: Option<String>,
}

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditFileTool {
    pub fn new() -> Self {
        Self {
            disallow_message: None,
        }
    }

    /// Mark this tool as disallowed. `execute()` will return the given message
    /// instead of performing the edit.
    pub fn disallow(mut self, message: impl Into<String>) -> Self {
        self.disallow_message = Some(message.into());
        self
    }
}

#[async_trait]
impl AgentTool for EditFileTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn label(&self) -> &str {
        "Edit File"
    }

    fn description(&self) -> &str {
        "Make precise file edits with exact text replacement, including multiple disjoint edits in one call.\n\
         \n\
         Each edits[].old_text is matched against the original file, not incrementally.\n\
         Do not include overlapping or nested edits. If two changes touch nearby lines, merge them.\n\
         Keep old_text as small as possible while still unique in the file."
    }

    fn parameter_aliases(&self) -> Option<crate::tools::validation::AliasMap> {
        Some(&[("path", &["file_path", "filePath", "file"] as &[&str])])
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative or absolute)"
                },
                "edits": {
                    "type": "array",
                    "description": "One or more targeted replacements. Each edit is matched against the original file, not incrementally. Do not include overlapping or nested edits. If two changes touch the same block or nearby lines, merge them into one edit instead.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": {
                                "type": "string",
                                "description": "Exact text for one targeted replacement. It must be unique in the original file and must not overlap with any other edits[].old_text in the same call."
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Replacement text for this targeted edit."
                            }
                        },
                        "required": ["old_text", "new_text"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        let n = params["edits"].as_array().map(|a| a.len()).unwrap_or(1);
        Some(format!("edit {path} ({n} replacement(s))"))
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(msg) = &self.disallow_message {
            return Err(ToolError::Failed(format!("Error: {msg}")));
        }

        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path' parameter".into()))?;
        let path = ctx.path_guard.resolve_path(&ctx.cwd, path_str)?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Read file bytes and validate UTF-8
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            ToolError::Failed(format!(
                "Cannot read {path_str}: {e}. Use Write to create new files."
            ))
        })?;
        let raw = String::from_utf8(bytes).map_err(|_| {
            ToolError::Failed(format!(
                "Cannot edit {path_str}: only UTF-8 text files are supported."
            ))
        })?;

        // Strip BOM, detect line endings, normalize to LF
        let (bom, content_raw) = normalize::strip_utf8_bom(&raw);
        let line_ending = normalize::detect_line_ending(content_raw);
        let content_lf = normalize::normalize_to_lf(content_raw);

        // Parse edits
        let edits = self.parse_edits(&params)?;

        // Resolve all matches against original content
        let mut resolved: Vec<(usize, usize, &str, String)> = Vec::with_capacity(edits.len());
        for (i, (old_text_lf, new_text_lf)) in edits.iter().enumerate() {
            let rm = matching::resolve_unique_match(&content_lf, old_text_lf)
                .map_err(|e| self.match_error(e, path_str, old_text_lf, &content_lf, i))?;
            let start = content_lf.find(&rm.actual_old_text).unwrap_or(0);
            let end = start + rm.actual_old_text.len();
            resolved.push((
                start,
                end,
                new_text_lf.as_str(),
                rm.kind.as_str().to_string(),
            ));
        }

        // Check for overlaps
        resolved.sort_by_key(|(start, _, _, _)| *start);
        for w in resolved.windows(2) {
            if w[0].1 > w[1].0 {
                return Err(ToolError::Failed(
                    "Edits overlap. Merge nearby changes into one edit.".into(),
                ));
            }
        }

        // Apply in reverse order to preserve offsets
        let mut new_content_lf = content_lf.clone();
        let match_kind = resolved
            .last()
            .map(|(_, _, _, k)| k.clone())
            .unwrap_or_else(|| "exact".to_string());
        let replacement_count = resolved.len();
        for (start, end, new_text, _) in resolved.into_iter().rev() {
            new_content_lf.replace_range(start..end, new_text);
        }

        // No-change detection
        if new_content_lf == content_lf {
            return Err(ToolError::Failed(format!(
                "No changes made to {path_str}. The replacement produced identical content."
            )));
        }

        // Generate diff (for details only, not sent to LLM)
        let diff_result = diff::unified_diff(&content_lf, &new_content_lf, path_str);

        // Restore BOM + original line endings and write back
        let final_content = format!(
            "{}{}",
            bom,
            normalize::restore_line_endings(&new_content_lf, line_ending)
        );
        tokio::fs::write(&path, &final_content)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot write {path_str}: {e}")))?;

        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("Updated {path_str}."),
            }],
            details: serde_json::json!({
                "path": path_str,
                "match_kind": match_kind,
                "replacement_count": replacement_count,
                "diff": diff_result.unified,
                "first_changed_line": diff_result.first_changed_line,
                "added_lines": diff_result.added_lines,
                "removed_lines": diff_result.removed_lines,
            }),
            retention: Retention::Normal,
        })
    }
}

impl EditFileTool {
    /// Parse edits from params.
    fn parse_edits(&self, params: &serde_json::Value) -> Result<Vec<(String, String)>, ToolError> {
        let arr = params["edits"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'edits' parameter".into()))?;
        if arr.is_empty() {
            return Err(ToolError::InvalidArgs("edits[] must not be empty".into()));
        }
        let mut edits = Vec::with_capacity(arr.len());
        for (i, entry) in arr.iter().enumerate() {
            let old = entry["old_text"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs(format!("edits[{i}] missing old_text")))?;
            let new = entry["new_text"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs(format!("edits[{i}] missing new_text")))?;
            let old_lf = normalize::normalize_to_lf(old);
            let new_lf = normalize::normalize_to_lf(new);
            if old_lf.is_empty() {
                return Err(ToolError::Failed(format!(
                    "edits[{i}].old_text must not be empty."
                )));
            }
            edits.push((old_lf, new_lf));
        }
        Ok(edits)
    }

    /// Format a match error with helpful hints.
    fn match_error(
        &self,
        e: MatchError,
        path_str: &str,
        old_text_lf: &str,
        content_lf: &str,
        idx: usize,
    ) -> ToolError {
        let prefix = if idx > 0 {
            format!("edits[{idx}]: ")
        } else {
            String::new()
        };
        match e {
            MatchError::EmptyOldText => {
                ToolError::Failed(format!("{prefix}old_text must not be empty."))
            }
            MatchError::NotFound => {
                let hint = matching::find_similar_text(content_lf, old_text_lf);
                let suffix = match hint {
                    Some(similar) => format!(
                        "\n\nDid you mean:\n```\n{similar}\n```\n\
                         Make sure old_text matches the current file content exactly."
                    ),
                    None => "\n\nTip: Use Read to see the current file contents.".into(),
                };
                ToolError::Failed(format!("{prefix}old_text not found in {path_str}.{suffix}"))
            }
            MatchError::NotUnique { count } => ToolError::Failed(format!(
                "{prefix}old_text matches {count} locations in {path_str}. \
                 Include more surrounding context to make the match unique."
            )),
        }
    }
}
