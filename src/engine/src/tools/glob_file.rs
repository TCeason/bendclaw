//! Glob file tool — discover files by path pattern.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::types::*;

/// Find files by glob pattern. Returns paths only, never file contents.
pub struct GlobFileTool {
    pub max_results: usize,
    pub timeout: Duration,
}

impl Default for GlobFileTool {
    fn default() -> Self {
        Self {
            max_results: 200,
            timeout: Duration::from_secs(10),
        }
    }
}

impl GlobFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentTool for GlobFileTool {
    fn name(&self) -> &str {
        "glob_file"
    }

    fn label(&self) -> &str {
        "Glob Files"
    }

    fn description(&self) -> &str {
        "Find files by glob pattern. Use this tool to discover candidate file paths before reading them.\n\
         \n\
         Usage:\n\
         - Use this tool instead of shell find/ls for file discovery.\n\
         - Returns matching file paths only; it never reads file contents.\n\
         - The pattern parameter supports glob syntax such as '*.rs', 'src/**/*.rs', or 'tests/**/*'.\n\
         - Respects .gitignore when ripgrep is available.\n\
         - Excludes common noise directories: target, .git, node_modules."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern for files to return, e.g. 'src/**/*.rs'"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search from (optional, defaults to current directory)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of file paths to return (optional)"
                },
                "reason": {
                    "type": "string",
                    "description": "Why these files are being discovered (optional)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let pattern = params["pattern"].as_str()?;
        let path = params["path"].as_str().unwrap_or(".");
        Some(format!("rg --files {} -g {}", path, shell_quote(pattern)))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let pattern = params["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'pattern' parameter".into()))?;
        let path_str = params["path"].as_str().unwrap_or(".");
        let limit = params["limit"]
            .as_u64()
            .map(|v| v as usize)
            .unwrap_or(self.max_results)
            .min(self.max_results);
        let reason = params["reason"].as_str();

        let path = ctx
            .path_guard
            .resolve_optional_path(&ctx.cwd, Some(path_str))?;

        if !path.exists() {
            return Err(ToolError::Failed(format!(
                "Directory not found: {}. Check the path and try again.",
                path.display()
            )));
        }

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        let mut cmd = if which_exists("rg") {
            build_rg_command(&path, pattern)
        } else {
            build_find_command(&path, pattern)
        };
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let result = tokio::select! {
            _ = ctx.cancel.cancelled() => return Err(ToolError::Cancelled),
            _ = tokio::time::sleep(self.timeout) => return Err(ToolError::Failed("File discovery timed out".into())),
            result = cmd.output() => result.map_err(|e| ToolError::Failed(format!("Failed to discover files: {e}")))?,
        };

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();

        if !result.status.success() && !stderr.trim().is_empty() {
            return Err(ToolError::Failed(format!(
                "File discovery failed: {stderr}"
            )));
        }

        let mut paths: Vec<String> = stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| display_path(line, &path, &ctx.cwd))
            .collect();
        paths.sort();
        paths.dedup();

        let total = paths.len();
        let truncated = total > limit;
        if truncated {
            paths.truncate(limit);
        }

        let text = if paths.is_empty() {
            format!("No files matched pattern '{pattern}'")
        } else if truncated {
            format!(
                "{}\n... (showing first {limit} of {total} files)",
                paths.join("\n")
            )
        } else {
            format!("{}\n({total} files)", paths.join("\n"))
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({
                "pattern": pattern,
                "path": path_str,
                "files": total,
                "truncated": truncated,
                "reason": reason,
            }),
            retention: Retention::Normal,
        })
    }
}

fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn build_rg_command(path: &std::path::Path, pattern: &str) -> Command {
    let mut cmd = Command::new("rg");
    cmd.arg("--files");
    cmd.arg(path);
    cmd.arg("--glob").arg(pattern);
    cmd.arg("--glob").arg("!target/**");
    cmd.arg("--glob").arg("!.git/**");
    cmd.arg("--glob").arg("!node_modules/**");
    cmd
}

fn build_find_command(path: &std::path::Path, pattern: &str) -> Command {
    let mut cmd = Command::new("find");
    cmd.arg(path);
    cmd.args(["-not", "-path", "*/target/*"]);
    cmd.args(["-not", "-path", "*/.git/*"]);
    cmd.args(["-not", "-path", "*/node_modules/*"]);
    cmd.arg("-type").arg("f");
    cmd.arg("-path").arg(glob_to_find_path(pattern));
    cmd
}

fn glob_to_find_path(pattern: &str) -> String {
    if pattern.starts_with("./") || pattern.starts_with('/') {
        pattern.to_string()
    } else {
        format!("*/{pattern}")
    }
}

fn display_path(line: &str, search_root: &std::path::Path, cwd: &std::path::Path) -> String {
    let p = PathBuf::from(line);
    let absolute = if p.is_absolute() {
        p
    } else {
        search_root.join(p)
    };
    absolute
        .strip_prefix(cwd)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| absolute.to_string_lossy().to_string())
}

fn shell_quote(value: &str) -> String {
    if value.contains(' ') || value.contains('*') || value.contains('?') {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}
