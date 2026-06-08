//! `grep` — recursive file-content search returning `path:line: text`.
//!
//! Walks the tree with the `ignore` crate (gitignore-aware) and searches each
//! file with ripgrep's own engine (`grep-searcher` + `grep-regex`), which
//! brings binary-file detection, encoding handling, and line counting. Always
//! returns line numbers, so the agent can jump straight to a location without
//! re-reading the file to locate code.

use async_trait::async_trait;

use super::cap_output;
use crate::types::*;

const MAX_MATCHES: usize = 200;

/// Content search across files.
#[derive(Default)]
pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentTool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "Grep".into())]
    }

    fn label(&self) -> &str {
        "Search Contents"
    }

    fn description(&self) -> &str {
        "Search file contents recursively for a regex pattern. Always returns line numbers; \
         output format is `path:line: matching text`. Respects .gitignore. There is no need to \
         use bash grep or rg — this tool already provides line numbers and is faster to scan. \
         Output is truncated to 200 matches or 50KB (whichever is hit first)."
    }

    fn prompt_snippet(&self) -> Option<&str> {
        Some("Search file contents by regex (returns path:line: text)")
    }

    fn prompt_guidelines(&self) -> Vec<&str> {
        Vec::new()
    }

    fn prefer_over(&self) -> Option<(&str, &str)> {
        Some(("search file contents", "bash grep or rg"))
    }

    fn parameter_aliases(&self) -> Option<crate::tools::validation::AliasMap> {
        Some(&[
            ("pattern", &["query", "regex", "search"] as &[&str]),
            ("path", &["dir", "directory", "file"] as &[&str]),
            ("include", &["glob", "filter", "include_pattern"]
                as &[&str]),
        ])
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in. Defaults to the working directory."
                },
                "include": {
                    "type": "string",
                    "description": "File glob filter, e.g. '*.rs' or '**/*.test.ts'. Optional."
                },
                "ignore_case": {
                    "type": "boolean",
                    "description": "Case-insensitive search. Defaults to false."
                },
                "reason": {
                    "type": "string",
                    "description": "Briefly explain why you chose this pattern and what you expect to find."
                }
            },
            "required": ["pattern"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let pattern = params["pattern"].as_str()?;
        let path = params["path"].as_str().unwrap_or(".");
        Some(format!("grep -rn {pattern:?} {path}"))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let pattern = params["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'pattern' parameter".into()))?
            .to_string();
        if pattern.is_empty() {
            return Err(ToolError::InvalidArgs("'pattern' must not be empty".into()));
        }
        let include = params["include"].as_str().map(str::to_string);
        let ignore_case = params["ignore_case"].as_bool().unwrap_or(false);

        let search_root = ctx
            .path_guard
            .resolve_optional_path(&ctx.cwd, params["path"].as_str())?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        let output = run_search(
            &search_root,
            &pattern,
            include.as_deref(),
            ignore_case,
            &ctx,
        )
        .await?;

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({ "pattern": pattern }),
            retention: Retention::Normal,
        })
    }
}

// SEARCH_IMPL

use std::path::Path;

use grep::regex::RegexMatcher;
use grep::searcher::sinks::UTF8;
use grep::searcher::BinaryDetection;
use grep::searcher::SearcherBuilder;

/// Gitignore-aware walk + ripgrep's own search engine, run off the async
/// runtime. Using grep-searcher gives binary-file detection, encoding
/// handling, and line counting for free — the same machinery ripgrep uses.
async fn run_search(
    root: &Path,
    pattern: &str,
    include: Option<&str>,
    ignore_case: bool,
    ctx: &ToolContext,
) -> Result<String, ToolError> {
    let matcher = grep::regex::RegexMatcherBuilder::new()
        .case_insensitive(ignore_case)
        .build(pattern)
        .map_err(|e| ToolError::InvalidArgs(format!("invalid regex: {e}")))?;

    let glob = match include {
        Some(g) => Some(
            globset::Glob::new(g)
                .map_err(|e| ToolError::InvalidArgs(format!("invalid include glob: {e}")))?
                .compile_matcher(),
        ),
        None => None,
    };

    let root = root.to_path_buf();
    let cancel = ctx.cancel.clone();
    // Walking + reading files is blocking; run it off the async runtime.
    tokio::task::spawn_blocking(move || search_tree(&root, &matcher, glob.as_ref(), &cancel))
        .await
        .map_err(|e| ToolError::Failed(format!("search task panicked: {e}")))?
}

fn search_tree(
    root: &Path,
    matcher: &RegexMatcher,
    glob: Option<&globset::GlobMatcher>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String, ToolError> {
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .build();

    let mut lines: Vec<String> = Vec::new();
    let mut capped = false;

    let walker = ignore::WalkBuilder::new(root).require_git(false).build();
    'outer: for entry in walker {
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if let Some(g) = glob {
            // Match against the path relative to the search root so a filter
            // like '*.rs' behaves intuitively regardless of nesting.
            let rel = path.strip_prefix(root).unwrap_or(path);
            if !g.is_match(rel) {
                continue;
            }
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();

        let mut hit_cap = false;
        let sink = UTF8(|line_no, text| {
            if lines.len() >= MAX_MATCHES {
                hit_cap = true;
                return Ok(false); // stop searching this file
            }
            lines.push(format!("{}:{}: {}", rel, line_no, text.trim_end()));
            Ok(true)
        });
        // Search errors (e.g. unreadable file) are non-fatal; skip the file.
        let _ = searcher.search_path(matcher, path, sink);
        if hit_cap {
            capped = true;
            break 'outer;
        }
    }

    if lines.is_empty() {
        return Ok("(no matches)".into());
    }
    if capped {
        lines.push(format!("... (capped at {MAX_MATCHES} matches)"));
    }
    Ok(cap_output(
        lines.join("\n"),
        ", refine the pattern for more",
    ))
}
