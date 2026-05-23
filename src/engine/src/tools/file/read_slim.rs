//! ReadSlim tool — token-efficient slimmed file reading.

use async_trait::async_trait;

use super::is_image_file;
use crate::types::*;

/// Read a source file in a compact form for token-efficient understanding.
pub struct ReadSlimFileTool {
    /// Max file size to read (prevents OOM)
    pub max_bytes: usize,
}

impl Default for ReadSlimFileTool {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024, // 1MB
        }
    }
}

impl ReadSlimFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

fn slim_line(line: &str) -> String {
    let trimmed_end = line.trim_end();
    let indent_len = trimmed_end
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    let body = &trimmed_end[indent_len..];
    let slim_indent = if indent_len == 0 {
        String::new()
    } else {
        " ".repeat((indent_len / 4).max(1))
    };
    format!("{slim_indent}{body}")
}

fn format_slim_output(
    lines: &[&str],
    start: usize,
    end: usize,
    total: usize,
    original_chars: usize,
    display_start: usize,
) -> String {
    let mut previous_blank = false;
    let mut slimmed_chars = 0usize;
    let mut numbered = Vec::new();

    for (i, line) in lines[start..end].iter().enumerate() {
        let slimmed = slim_line(line);
        let is_blank = slimmed.trim().is_empty();
        if is_blank && previous_blank {
            continue;
        }
        previous_blank = is_blank;
        slimmed_chars += slimmed.len();
        numbered.push(format!("{:>4} | {}", display_start + i + 1, slimmed));
    }

    let saved_pct = if original_chars > 0 {
        ((original_chars - slimmed_chars) * 100) / original_chars
    } else {
        0
    };

    let header = if start > 0 || end < total {
        format!(
            "[read_slim_file: lines {}-{} of {} | original={} chars | slimmed={} chars | saved={}%, output is not exact]",
            display_start + 1,
            display_start + (end - start),
            total,
            original_chars,
            slimmed_chars,
            saved_pct
        )
    } else {
        format!(
            "[read_slim_file: {} lines | original={} chars | slimmed={} chars | saved={}%, output is not exact]",
            total, original_chars, slimmed_chars, saved_pct
        )
    };

    format!(
        "{}\n[Note: output is approximate. Use Read with offset/limit to get exact text before editing.]\n{}",
        header,
        numbered.join("\n")
    )
}

#[async_trait]
impl AgentTool for ReadSlimFileTool {
    fn name(&self) -> &str {
        "ReadSlim"
    }

    fn label(&self) -> &str {
        "Read Slim File"
    }

    fn description(&self) -> &str {
        "Read a source file in a token-efficient slimmed form for understanding code structure and logic.\n\
         \n\
         Usage:\n\
         - The path parameter should be an absolute path, not a relative path.\n\
         - Use this tool for large source files when you only need to understand structure, control flow, or relationships.\n\
         - The returned content is not exact and must not be used as old_text for Edit.\n\
         - Before editing, use Read with offset/limit around the target code to get exact text.\n\
         - Use Read for Markdown, snapshots, generated text, or anything where formatting matters.\n\
         - Supports optional offset/limit for partial reads of large files.\n\
         To discover files, use Glob."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Source file path to read in slim form"
                },
                "offset": {
                    "type": "integer",
                    "description": "Starting line number (1-indexed, optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return (optional)"
                },
                "reason": {
                    "type": "string",
                    "description": "Why this file is being read (optional)"
                }
            },
            "required": ["path"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        let offset = params["offset"].as_u64();
        let limit = params["limit"].as_u64();
        match (offset, limit) {
            (Some(off), Some(lim)) => {
                let end = off.saturating_add(lim).saturating_sub(1);
                Some(format!("read_slim_file {} lines {}-{}", path, off, end))
            }
            (Some(off), None) => Some(format!("read_slim_file {} from line {}", path, off)),
            (None, Some(lim)) => Some(format!("read_slim_file {} first {} lines", path, lim)),
            (None, None) => Some(format!("read_slim_file {}", path)),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path' parameter".into()))?;
        let reason = params["reason"].as_str();

        let path = ctx.path_guard.resolve_path(&ctx.cwd, path_str)?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot access {}: {}", path.display(), e)))?;

        if is_image_file(&path) {
            return Err(ToolError::Failed(
                "read_slim_file only supports text source files. Use Read for images.".into(),
            ));
        }

        let offset = params["offset"].as_u64().map(|v| v.max(1) as usize);
        let limit = params["limit"].as_u64().map(|v| v as usize);

        if metadata.len() as usize > self.max_bytes {
            let Some(lim) = limit else {
                return Err(ToolError::Failed(format!(
                    "File too large ({} bytes, max {}). Use offset and limit for partial slim reads.",
                    metadata.len(),
                    self.max_bytes
                )));
            };
            return self
                .read_slim_lines_streaming(&path, path_str, offset.unwrap_or(1), lim, reason)
                .await;
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path.display(), e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let (start, end) = match (offset, limit) {
            (Some(off), Some(lim)) => {
                let s = (off - 1).min(total);
                (s, (s + lim).min(total))
            }
            (Some(off), None) => {
                let s = (off - 1).min(total);
                (s, total)
            }
            (None, Some(lim)) => (0, lim.min(total)),
            (None, None) => (0, total),
        };

        let original_chars = lines[start..end].iter().map(|line| line.len()).sum();
        let output = format_slim_output(&lines, start, end, total, original_chars, start);

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({
                "path": path_str,
                "reason": reason,
                "exact": false,
            }),
            retention: Retention::Normal,
        })
    }
}

impl ReadSlimFileTool {
    async fn read_slim_lines_streaming(
        &self,
        path: &std::path::Path,
        path_str: &str,
        offset: usize,
        limit: usize,
        reason: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        use tokio::io::AsyncBufReadExt;
        use tokio::io::BufReader;

        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path.display(), e)))?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let start = offset.saturating_sub(1);
        let end = start + limit;
        let mut collected = Vec::with_capacity(limit);
        let mut original_chars = 0usize;
        let mut line_num = 0usize;

        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| ToolError::Failed(format!("Read error: {e}")))?
        {
            if line_num >= end {
                break;
            }
            if line_num >= start {
                original_chars += line.len();
                collected.push(line);
            }
            line_num += 1;
        }

        let refs: Vec<&str> = collected.iter().map(String::as_str).collect();
        let output = format_slim_output(&refs, 0, refs.len(), line_num, original_chars, start);

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({
                "path": path_str,
                "reason": reason,
                "exact": false,
            }),
            retention: Retention::Normal,
        })
    }
}
