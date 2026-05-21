//! File tools — read and write files with safety limits.

use std::path::Path;

use async_trait::async_trait;

use super::edit::diff;
use crate::types::*;

/// 20 MB limit for image files
const MAX_IMAGE_SIZE_BYTES: u64 = 20 * 1024 * 1024;

fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp")
    )
}

fn get_image_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
}

/// Read a file's contents. Supports line range for large files.
pub struct ReadFileTool {
    /// Max file size to read (prevents OOM)
    pub max_bytes: usize,
}

/// Read a source file in a compact form for token-efficient understanding.
pub struct ReadSlimFileTool {
    /// Max file size to read (prevents OOM)
    pub max_bytes: usize,
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

    let saved = if original_chars == 0 {
        0
    } else {
        100usize.saturating_sub((slimmed_chars * 100) / original_chars)
    };
    let actual_end = end.min(start + numbered.len());
    let header = if start > 0 || actual_end < total {
        format!(
            "[read_slim_file: lines {}-{} of {} | original={} chars | slimmed={} chars | saved={}%, output is not exact]",
            start + 1,
            actual_end,
            total,
            original_chars,
            slimmed_chars,
            saved
        )
    } else {
        format!(
            "[read_slim_file: {} lines | original={} chars | slimmed={} chars | saved={}%, output is not exact]",
            total, original_chars, slimmed_chars, saved
        )
    };

    format!(
        "{}\nWARNING: read_slim_file output is not exact. Do not use it as old_text for edit_file; use read_file with offset/limit for exact text before editing.\n{}",
        header,
        numbered.join("\n")
    )
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024, // 1MB
        }
    }
}

impl Default for ReadSlimFileTool {
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

impl ReadSlimFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn label(&self) -> &str {
        "Read File"
    }

    fn description(&self) -> &str {
        "Read a text file from the local filesystem and return exact original content. You can access any file directly by using this tool.\n\
         \n\
         Usage:\n\
         - The path parameter should be an absolute path, not a relative path.\n\
         - Use this tool when exact text matters, especially before edit_file.\n\
         - Use read_slim_file for large source files when you only need to understand structure or logic.\n\
         - Use this tool instead of shell cat/head/tail/sed -n for exact file reads.\n\
         - Supports optional offset/limit for partial reads of large files.\n\
         - This tool can only read text files and images (jpg, png, webp, gif, bmp), not directories or binary files.\n\
         To discover files, use glob_file.\n\
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

        // Handle image files: keep path in context and resolve bytes only when sent to provider.
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
            if result.is_ok()
                && ctx
                    .spill
                    .as_ref()
                    .is_some_and(|spill| spill.contains_path(&path))
            {
                if let Some(progress) = &ctx.on_progress {
                    progress(
                        SpillProgress::read(
                            path.to_string_lossy(),
                            metadata.len() as usize,
                            read_start.elapsed().as_millis() as u64,
                        )
                        .to_progress_text(),
                    );
                }
            }
            return result;
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path.display(), e)))?;

        // Always show line numbers — helps agent reference exact lines for edit_file
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

#[async_trait]
impl AgentTool for ReadSlimFileTool {
    fn name(&self) -> &str {
        "read_slim_file"
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
         - The returned content is not exact and must not be used as old_text for edit_file.\n\
         - Before editing, use read_file with offset/limit around the target code to get exact text.\n\
         - Use read_file for Markdown, snapshots, generated text, or anything where formatting matters.\n\
         - Supports optional offset/limit for partial reads of large files.\n\
         To discover files, use glob_file."
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
                "read_slim_file only supports text source files. Use read_file for images.".into(),
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

// ---------------------------------------------------------------------------

/// Write content to a file. Creates parent directories if needed.
pub struct WriteFileTool {
    disallow_message: Option<String>,
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self {
            disallow_message: None,
        }
    }

    /// Mark this tool as disallowed. `execute()` will return the given message
    /// instead of performing the write.
    pub fn disallow(mut self, message: impl Into<String>) -> Self {
        self.disallow_message = Some(message.into());
        self
    }
}

#[async_trait]
impl AgentTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn label(&self) -> &str {
        "Write File"
    }

    fn description(&self) -> &str {
        "Write contents to a file on the local filesystem.\n\
         \n\
         Usage:\n\
         - This tool will overwrite the existing file if there is one at the provided path.\n\
         - If this is an existing file, you MUST use read_file first to read the file's contents. \
         This tool will fail if you did not read the file first.\n\
         - Prefer edit_file for modifying existing files — it only sends the diff. \
         Only use this tool to create new files or for complete rewrites.\n\
         - Creates parent directories automatically if they don't exist."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                },
                "reason": {
                    "type": "string",
                    "description": "Why this file is being written (optional)"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        Some(format!("cat > {}", path))
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
        let content = params["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'content' parameter".into()))?;
        let reason = params["reason"].as_str();

        let path = ctx.path_guard.resolve_path(&ctx.cwd, path_str)?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Read old content before writing (for diff display)
        let old_content = tokio::fs::read_to_string(&path).await.ok();

        // Create parent directories
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| ToolError::Failed(format!("Cannot create directory: {}", e)))?;
            }
        }

        tokio::fs::write(&path, content)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot write {}: {}", path.display(), e)))?;

        let bytes = content.len();
        let existed = old_content.is_some();
        let old = old_content.as_deref().unwrap_or("");
        let diff_result = diff::unified_diff(old, content, path_str);
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("Wrote {} bytes to {}", bytes, path_str),
            }],
            details: serde_json::json!({
                "path": path_str,
                "bytes": bytes,
                "created": !existed,
                "diff": diff_result.unified,
                "reason": reason,
            }),
            retention: Retention::Normal,
        })
    }
}
