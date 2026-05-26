//! Read tool — read exact file contents.

use async_trait::async_trait;

use super::get_image_mime_type;
use super::is_image_file;
use super::MAX_IMAGE_SIZE_BYTES;
use crate::types::*;

/// Read a file's contents. Supports line range for large files.
pub struct ReadFileTool {
    /// Max file size to read (prevents OOM)
    pub max_bytes: usize,
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024, // 1MB
        }
    }
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentTool for ReadFileTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn label(&self) -> &str {
        "Read File"
    }

    fn description(&self) -> &str {
        "Read a text file from the local filesystem and return exact original content. You can access any file directly by using this tool.\n\
         \n\
         Usage:\n\
         - The path parameter should be an absolute path, not a relative path.\n\
         - Use this tool when exact text matters, especially before Edit.\n\
         - Use this tool instead of shell cat/head/tail/sed -n for exact file reads.\n\
         - Supports optional offset/limit for partial reads of large files.\n\
         - This tool can only read text files and images (jpg, png, webp, gif, bmp), not directories or binary files.\n\
         To discover files, use Glob.\n\
         - If you read a file that exists but has empty contents you will receive a warning \
         in place of file contents."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to read"
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
                Some(format!("sed -n '{},{}p' {}", off, end, path))
            }
            (Some(off), None) => Some(format!("sed -n '{},$p' {}", off, path)),
            (None, Some(lim)) => Some(format!("head -n {} {}", lim, path)),
            (None, None) => Some(format!("cat -n {}", path)),
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

        // Check file exists and size
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot access {}: {}", path.display(), e)))?;

        // Handle image files
        if is_image_file(&path) {
            if metadata.len() > MAX_IMAGE_SIZE_BYTES {
                return Err(ToolError::Failed(format!(
                    "Image too large ({}MB, max 20MB)",
                    metadata.len() / (1024 * 1024)
                )));
            }
            let mime_type = get_image_mime_type(&path)
                .ok_or_else(|| ToolError::Failed("Unknown image format".into()))?;
            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path.display(), e)))?;
            return Ok(ToolResult {
                content: vec![Content::Image {
                    mime_type: mime_type.to_string(),
                    source: ImageSource::Path {
                        path: path.to_string_lossy().to_string(),
                    },
                }],
                details: serde_json::json!({
                    "path": path_str,
                    "bytes": meta.len(),
                    "reason": reason,
                }),
                retention: Retention::Normal,
            });
        }

        // Text files: check size limit and apply line offset/limit
        let offset = params["offset"].as_u64().map(|v| v.max(1) as usize);
        let limit = params["limit"].as_u64().map(|v| v as usize);

        if metadata.len() as usize > self.max_bytes {
            let Some(lim) = limit else {
                return Err(ToolError::Failed(format!(
                    "File too large ({} bytes, max {}). Use offset and limit for partial reads.",
                    metadata.len(),
                    self.max_bytes
                )));
            };
            let read_start = std::time::Instant::now();
            let result = self
                .read_lines_streaming(&path, path_str, offset.unwrap_or(1), lim)
                .await;
            if let Ok(ref res) = result {
                if ctx
                    .spill
                    .as_ref()
                    .is_some_and(|spill| spill.contains_path(&path))
                {
                    if let Some(progress) = &ctx.on_progress {
                        let actual_bytes = res
                            .content
                            .iter()
                            .map(|c| match c {
                                Content::Text { text } => text.len(),
                                _ => 0,
                            })
                            .sum::<usize>();
                        progress(
                            SpillProgress::read(
                                path.to_string_lossy(),
                                actual_bytes,
                                read_start.elapsed().as_millis() as u64,
                            )
                            .to_progress_text(),
                        );
                    }
                }
            }
            return result;
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path.display(), e)))?;

        // Always show line numbers — helps agent reference exact lines
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

        let numbered: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>4} | {}", start + i + 1, line))
            .collect();

        let header = if start > 0 || end < total {
            format!("[Lines {}-{} of {}]", start + 1, end, total)
        } else {
            format!("[{} lines]", total)
        };

        let output = format!("{}\n{}", header, numbered.join("\n"));

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({
                "path": path_str,
                "reason": reason,
            }),
            retention: Retention::Normal,
        })
    }
}

impl ReadFileTool {
    /// Stream-read a large file by lines, only collecting the requested range.
    async fn read_lines_streaming(
        &self,
        path: &std::path::Path,
        path_str: &str,
        offset: usize,
        limit: usize,
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
        let mut collected: Vec<String> = Vec::with_capacity(limit);
        let mut line_num: usize = 0;

        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| ToolError::Failed(format!("Read error: {e}")))?
        {
            if line_num >= end {
                break;
            }
            if line_num >= start {
                collected.push(format!("{:>4} | {}", line_num + 1, line));
            }
            line_num += 1;
        }

        let header = format!("[Lines {}-{}]", start + 1, start + collected.len());
        let output = format!("{}\n{}", header, collected.join("\n"));

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({ "path": path_str }),
            retention: Retention::Normal,
        })
    }
}
