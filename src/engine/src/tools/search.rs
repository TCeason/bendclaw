//! Grep tool — content search across local files.

use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::types::*;

/// Search file contents using grep (or ripgrep if available).
pub struct SearchTool {
    /// Root directory to search in
    pub root: Option<String>,
    /// Max results to return
    pub max_results: usize,
    /// Timeout
    pub timeout: Duration,
}

impl Default for SearchTool {
    fn default() -> Self {
        Self {
            root: None,
            max_results: 50,
            timeout: Duration::from_secs(30),
        }
    }
}

impl SearchTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_root(mut self, root: impl Into<String>) -> Self {
        self.root = Some(root.into());
        self
    }
}

#[async_trait]
impl AgentTool for SearchTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn label(&self) -> &str {
        "Grep Files"
    }

    fn description(&self) -> &str {
        "Search local file contents with grep/ripgrep.\n\
         \n\
         Usage:\n\
         - Use this tool for local content search. Do not use it to discover files by path; use Glob for that.\n\
         - Supports full regex syntax (e.g., \"log.*Error\", \"function\\s+\\w+\").\n\
         - Filter files with the include parameter (e.g., \"*.rs\", \"*.py\").\n\
         - Respects .gitignore when ripgrep is available. Returns matching lines with file paths and line numbers."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Content search pattern (regex supported)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (optional, defaults to working directory)"
                },
                "include": {
                    "type": "string",
                    "description": "File glob pattern to include, e.g. '*.rs' (optional)"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Case sensitive search (default: false)"
                },
                "reason": {
                    "type": "string",
                    "description": "Why this content search is being performed (optional)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let pattern = params["pattern"].as_str()?;
        let search_path = params["path"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| self.root.clone())
            .unwrap_or_else(|| ".".into());
        let include = params["include"].as_str();
        let case_sensitive = params["case_sensitive"].as_bool().unwrap_or(false);

        let (cmd_name, args) = if which_exists("rg") {
            build_rg_args(
                pattern,
                &search_path,
                include,
                case_sensitive,
                self.max_results,
            )
        } else {
            build_grep_args(
                pattern,
                &search_path,
                include,
                case_sensitive,
                self.max_results,
            )
        };

        Some(format!("{} {}", cmd_name, shell_join(&args)))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let cancel = ctx.cancel;
        let pattern = params["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'pattern' parameter".into()))?;

        let search_path_str = params["path"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| self.root.clone())
            .unwrap_or_else(|| ".".into());

        let include = params["include"].as_str();
        let case_sensitive = params["case_sensitive"].as_bool().unwrap_or(false);
        let reason = params["reason"].as_str();

        let search_path = ctx
            .path_guard
            .resolve_optional_path(&ctx.cwd, Some(&search_path_str))?;

        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Try ripgrep first, fall back to grep
        let search_path_resolved = search_path.to_string_lossy();
        let (cmd_name, args) = if which_exists("rg") {
            build_rg_args(
                pattern,
                &search_path_resolved,
                include,
                case_sensitive,
                self.max_results,
            )
        } else {
            build_grep_args(
                pattern,
                &search_path_resolved,
                include,
                case_sensitive,
                self.max_results,
            )
        };

        let mut cmd = Command::new(&cmd_name);
        cmd.args(&args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let timeout = self.timeout;

        let result = tokio::select! {
            _ = cancel.cancelled() => {
                return Err(ToolError::Cancelled);
            }
            _ = tokio::time::sleep(timeout) => {
                return Err(ToolError::Failed("Search timed out".into()));
            }
            result = cmd.output() => {
                result.map_err(|e| ToolError::Failed(format!("Search failed: {}", e)))?
            }
        };

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();

        // grep returns exit code 1 for "no matches" — that's not an error
        if result.status.code() == Some(2)
            || (!stderr.is_empty() && result.status.code() != Some(1))
        {
            return Err(ToolError::Failed(format!("Search error: {}", stderr)));
        }

        if stdout.trim().is_empty() {
            return Ok(ToolResult {
                content: vec![Content::Text {
                    text: format!("No matches found for '{}'", pattern),
                }],
                details: serde_json::json!({
                    "matches": 0,
                    "pattern": pattern,
                    "path": search_path_str,
                    "reason": reason,
                }),
                retention: Retention::Normal,
            });
        }

        let match_count = stdout.lines().count();
        let text = if match_count >= self.max_results {
            format!(
                "{}\n... (showing first {} matches)",
                stdout.trim(),
                self.max_results
            )
        } else {
            format!("{}\n({} matches)", stdout.trim(), match_count)
        };

        Ok(ToolResult {
            content: vec![Content::Text { text }],
            details: serde_json::json!({
                "matches": match_count,
                "pattern": pattern,
                "path": search_path_str,
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

fn build_rg_args(
    pattern: &str,
    path: &str,
    include: Option<&str>,
    case_sensitive: bool,
    max_results: usize,
) -> (String, Vec<String>) {
    let mut args = vec![
        "--line-number".into(),
        "--no-heading".into(),
        format!("--max-count={}", max_results),
    ];

    if !case_sensitive {
        args.push("--ignore-case".into());
    }

    if let Some(glob) = include {
        args.push(format!("--glob={}", glob));
    }

    args.push(pattern.into());
    args.push(path.into());

    ("rg".into(), args)
}

fn build_grep_args(
    pattern: &str,
    path: &str,
    include: Option<&str>,
    case_sensitive: bool,
    max_results: usize,
) -> (String, Vec<String>) {
    let mut args = vec!["-r".into(), "-n".into(), format!("-m{}", max_results)];

    if !case_sensitive {
        args.push("-i".into());
    }

    if let Some(glob) = include {
        args.push(format!("--include={}", glob));
    }

    args.push(pattern.into());
    args.push(path.into());

    ("Grep".into(), args)
}

/// Join args into a shell-safe display string.
/// Args containing spaces or special chars are quoted.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.contains(' ') || a.contains('*') || a.contains('?') {
                format!("\"{}\"", a)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
