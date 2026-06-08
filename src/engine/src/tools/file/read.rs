//! Read tool — read exact file contents.

use async_trait::async_trait;

use super::image::get_image_mime_type;
use super::image::is_image_file;
use super::image::MAX_IMAGE_SIZE_BYTES;
use crate::types::*;

/// Max lines returned by a single Read call (matches industry standard).
const MAX_READ_LINES: usize = 2000;
/// Max bytes returned by a single Read call.
const MAX_READ_BYTES: usize = 50 * 1024; // 50KB

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
        "read"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "Read".into())]
    }

    fn label(&self) -> &str {
        "Read File"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Supports text files and images (jpg, png, webp, gif, bmp). \
         For text files, output is truncated to 2000 lines or 50KB (whichever is hit first). \
         Use offset/limit for large files. When you need the full file, continue with offset until complete."
    }

    fn prompt_snippet(&self) -> Option<&str> {
        Some("Read file contents")
    }

    fn prompt_guidelines(&self) -> Vec<&str> {
        Vec::new()
    }

    fn prefer_over(&self) -> Option<(&str, &str)> {
        Some(("read or examine files", "cat, head, tail, or sed"))
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
                    "description": "Path to the file to read (relative or absolute)"
                },
                "offset": {
                    "type": "number",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of lines to read"
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

        // Apply truncation limits (2000 lines / 50KB) like pi does.
        let selected_lines = &lines[start..end];
        let (truncated_end, truncated_by) = truncate_selected(selected_lines, start, end, total);

        let numbered: Vec<String> = lines[start..truncated_end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>4} | {}", start + i + 1, line))
            .collect();

        let mut output = if start > 0 || truncated_end < total {
            format!(
                "[Lines {}-{} of {}]\n{}",
                start + 1,
                truncated_end,
                total,
                numbered.join("\n")
            )
        } else {
            format!("[{} lines]\n{}", total, numbered.join("\n"))
        };

        if let Some(reason_str) = truncated_by {
            let next_offset = truncated_end + 1;
            output.push_str(&format!(
                "\n\n[Showing lines {}-{} of {} ({} limit). Use offset={} to continue.]",
                start + 1,
                truncated_end,
                total,
                reason_str,
                next_offset
            ));
        }

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({
                "path": path_str,
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

/// Determine how many lines to actually return, respecting MAX_READ_LINES and MAX_READ_BYTES.
/// Returns (actual_end_index, truncation_reason) where truncation_reason is None if not truncated.
fn truncate_selected(
    lines: &[&str],
    start: usize,
    _end: usize,
    _total: usize,
) -> (usize, Option<&'static str>) {
    let count = lines.len();

    // Check line limit first
    if count > MAX_READ_LINES {
        return (start + MAX_READ_LINES, Some("2000 line"));
    }

    // Check byte limit (use UTF-8 byte length for correct CJK handling)
    let mut byte_count = 0usize;
    for (i, line) in lines.iter().enumerate() {
        // Account for line number prefix "{:>4} | " = 7 bytes + line UTF-8 bytes + newline
        byte_count += 7 + line.len() + 1;
        if byte_count > MAX_READ_BYTES {
            let truncated_end = start + i;
            if truncated_end > start {
                return (truncated_end, Some("50KB"));
            }
            return (start + 1, Some("50KB"));
        }
    }

    (start + count, None)
}
