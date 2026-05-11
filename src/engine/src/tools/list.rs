//! List files tool — directory exploration.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::types::*;

/// Directory file count threshold before switching to compact grouped output.
const SLIM_THRESHOLD: usize = 80;
/// Maximum individual file paths to show in compact grouped output.
const SLIM_MAX_FILES: usize = 80;
/// Maximum files shown per directory group in compact output.
const SLIM_MAX_FILES_PER_DIR: usize = 8;

/// List files and directories. Uses `find` or `fd` for efficient traversal.
pub struct ListFilesTool {
    pub max_results: usize,
    pub timeout: Duration,
}

impl Default for ListFilesTool {
    fn default() -> Self {
        Self {
            max_results: 200,
            timeout: Duration::from_secs(10),
        }
    }
}

impl ListFilesTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentTool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn label(&self) -> &str {
        "List Files"
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Use to explore project structure before reading specific files.\n\
         \n\
         Usage:\n\
         - Use this tool instead of shell ls or find for directory listing.\n\
         - Optionally filter by glob pattern (e.g., '*.rs').\n\
         - Excludes common noise directories (target, .git, node_modules) by default.\n\
         - If your command will create new directories or files, first use this tool to verify the \
         parent directory exists and is the correct location."
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str().unwrap_or(".");
        let max_depth = params["max_depth"].as_u64().unwrap_or(3);
        let pattern = params["pattern"].as_str();

        let mut parts = vec![
            "find".into(),
            path.to_string(),
            format!("-maxdepth {max_depth}"),
        ];
        if let Some(pat) = pattern {
            parts.push(format!("-name {pat}"));
        }
        parts.push(r#"-not -path "*/target/*""#.into());
        parts.push(r#"-not -path "*/.git/*""#.into());
        parts.push(r#"-not -path "*/node_modules/*""#.into());
        parts.push("-type f".into());

        Some(parts.join(" "))
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (default: current directory)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files, e.g. '*.rs' (optional)"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum directory depth (default: 3)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let cancel = ctx.cancel;
        let path_str = params["path"].as_str().unwrap_or(".");
        let pattern = params["pattern"].as_str();
        let max_depth = params["max_depth"].as_u64().unwrap_or(3);

        let path = ctx
            .path_guard
            .resolve_optional_path(&ctx.cwd, Some(path_str))?;

        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Check path exists
        if !path.exists() {
            return Err(ToolError::Failed(format!(
                "Directory not found: {}. Check the path and try again.",
                path.display()
            )));
        }

        let mut cmd = Command::new("find");
        cmd.arg(&path);
        cmd.args(["-maxdepth", &max_depth.to_string()]);

        if let Some(pat) = pattern {
            cmd.args(["-name", pat]);
        }

        // Exclude common noise
        cmd.args(["-not", "-path", "*/target/*"]);
        cmd.args(["-not", "-path", "*/.git/*"]);
        cmd.args(["-not", "-path", "*/node_modules/*"]);

        cmd.arg("-type").arg("f");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let timeout = self.timeout;

        let result = tokio::select! {
            _ = cancel.cancelled() => return Err(ToolError::Cancelled),
            _ = tokio::time::sleep(timeout) => return Err(ToolError::Failed("Listing timed out".into())),
            result = cmd.output() => {
                result.map_err(|e| ToolError::Failed(format!("Failed to list: {}", e)))?
            }
        };

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let mut lines: Vec<&str> = stdout.lines().collect();
        lines.sort();

        let total = lines.len();
        let truncated = total > self.max_results;
        if truncated {
            lines.truncate(self.max_results);
        }

        let (text, slimmed) = if lines.is_empty() {
            (format!("No files found in {}", path.display()), false)
        } else if total > SLIM_THRESHOLD || truncated {
            (
                format_slim_list(&lines, total, self.max_results, truncated),
                true,
            )
        } else {
            (format!("{}\n\n({} files)", lines.join("\n"), total), false)
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({ "total": total, "truncated": truncated, "slimmed": slimmed }),
            retention: Retention::Normal,
        })
    }
}

fn format_slim_list(files: &[&str], total: usize, max_results: usize, truncated: bool) -> String {
    let mut by_dir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for file in files {
        let path = Path::new(file);
        let dir = path.parent().and_then(Path::to_str).unwrap_or(".");
        by_dir.entry(dir).or_default().push(file);
    }

    let truncation = if truncated {
        format!(", showing first {max_results}")
    } else {
        String::new()
    };
    let mut output = format!("[{total} files{truncation}; compact grouped view]\n");

    let mut shown = 0usize;
    for (dir, names) in by_dir {
        if shown >= SLIM_MAX_FILES {
            break;
        }

        let remaining = SLIM_MAX_FILES - shown;
        let take = names.len().min(SLIM_MAX_FILES_PER_DIR).min(remaining);
        output.push_str(&format!("{dir} ({})\n", names.len()));

        for name in names.iter().take(take) {
            output.push_str("  ");
            output.push_str(name);
            output.push('\n');
        }

        shown += take;
        if names.len() > take {
            output.push_str(&format!("  ... {} more\n", names.len() - take));
        }
    }

    let hidden = files.len().saturating_sub(shown);
    if hidden > 0 {
        output.push_str(&format!("\n... {hidden} more shown files omitted"));
    }
    if truncated {
        output.push_str(&format!(
            "\n... {} total files omitted",
            total.saturating_sub(files.len())
        ));
    }

    output
}
