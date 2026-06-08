//! `glob` — find file paths matching one or more glob patterns.
//!
//! Walks the tree in-process with the `ignore` crate (gitignore-aware) and
//! matches names with `globset` — the same engine fd is built on. Accepts
//! arrays of patterns and paths, unioned in a single call, so the agent can
//! locate many file kinds in one round-trip instead of several.

use async_trait::async_trait;

use super::cap_output;
use crate::types::*;

const DEFAULT_MAX_RESULTS: usize = 1000;

/// File-name search by glob pattern.
#[derive(Default)]
pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentTool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "Glob".into())]
    }

    fn label(&self) -> &str {
        "Find Files"
    }

    fn description(&self) -> &str {
        "Find file paths matching one or more glob patterns. Returns paths sorted alphabetically \
         and deduplicated, relative to the search root. Respects .gitignore. There is no need to \
         use bash find or ls — prefer one call with several patterns over multiple calls. Use \
         '**' to recurse, '*' to match one path segment. Examples: ['**/*.rs'], \
         ['Cargo.toml', 'src/**/*.rs']. Output is truncated to 1000 results or 50KB."
    }

    fn prompt_snippet(&self) -> Option<&str> {
        Some("Find files by glob pattern (prefer one call with many patterns)")
    }

    fn prompt_guidelines(&self) -> Vec<&str> {
        vec![
            "When using glob, pass several patterns in one call rather than making separate calls.",
        ]
    }

    fn prefer_over(&self) -> Option<(&str, &str)> {
        Some(("find files by name", "bash find or ls"))
    }

    fn parameter_aliases(&self) -> Option<crate::tools::validation::AliasMap> {
        Some(&[
            ("pattern", &["patterns", "glob", "query"] as &[&str]),
            ("path", &["paths", "dir", "directory"] as &[&str]),
        ])
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "description": "Glob pattern(s), unioned. Use '**' to recurse. Always an array, even for one pattern, e.g. ['**/*.rs']."
                },
                "path": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Base directories to search within. Defaults to the working directory."
                },
                "type": {
                    "type": "string",
                    "enum": ["f", "d", "any"],
                    "description": "Filter by entry type: 'f' files, 'd' directories, 'any' both. Defaults to 'f'."
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of results. Defaults to 1000."
                },
                "reason": {
                    "type": "string",
                    "description": "Briefly explain which patterns you chose and what you expect to find."
                }
            },
            "required": ["pattern"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let pats = collect_strings(&params["pattern"]);
        if pats.is_empty() {
            return None;
        }
        Some(format!("find . -name {:?}", pats.join(" -o -name ")))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let patterns = collect_strings(&params["pattern"]);
        if patterns.is_empty() {
            return Err(ToolError::InvalidArgs(
                "'pattern' is required (a non-empty array of glob strings)".into(),
            ));
        }
        let type_filter = params["type"].as_str().unwrap_or("f").to_string();
        let max_results = params["max_results"]
            .as_u64()
            .map(|n| n as usize)
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_RESULTS);

        // Resolve base dirs; default to cwd when none given.
        let raw_paths = collect_strings(&params["path"]);
        let mut roots = Vec::new();
        if raw_paths.is_empty() {
            roots.push(ctx.path_guard.resolve_optional_path(&ctx.cwd, None)?);
        } else {
            for p in &raw_paths {
                roots.push(ctx.path_guard.resolve_path(&ctx.cwd, p)?);
            }
        }

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        let output = run_glob(&roots, &patterns, &type_filter, max_results, &ctx).await?;

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({ "patterns": patterns }),
            retention: Retention::Normal,
        })
    }
}

/// Normalize a JSON value into a list of non-empty strings. Accepts a single
/// string or an array of strings, so the model can pass either form.
fn collect_strings(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::String(s) if !s.is_empty() => vec![s.clone()],
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

// GLOB_IMPL

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Gitignore-aware walk + globset match, run off the async runtime.
async fn run_glob(
    roots: &[PathBuf],
    patterns: &[String],
    type_filter: &str,
    max_results: usize,
    ctx: &ToolContext,
) -> Result<String, ToolError> {
    let mut builder = globset::GlobSetBuilder::new();
    for pat in patterns {
        builder.add(
            globset::Glob::new(pat)
                .map_err(|e| ToolError::InvalidArgs(format!("invalid glob '{pat}': {e}")))?,
        );
    }
    let set = builder
        .build()
        .map_err(|e| ToolError::InvalidArgs(format!("invalid glob set: {e}")))?;

    let roots = roots.to_vec();
    let type_filter = type_filter.to_string();
    let cancel = ctx.cancel.clone();
    let matches = tokio::task::spawn_blocking(move || {
        builtin_walk(&roots, &set, &type_filter, max_results, &cancel)
    })
    .await
    .map_err(|e| ToolError::Failed(format!("glob task panicked: {e}")))??;

    Ok(format_matches(matches, max_results))
}

fn builtin_walk(
    roots: &[PathBuf],
    set: &globset::GlobSet,
    type_filter: &str,
    max_results: usize,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<BTreeSet<String>, ToolError> {
    let mut matches: BTreeSet<String> = BTreeSet::new();
    // With a single root, render paths relative to it (the common case). With
    // several roots, render absolute paths so entries stay unambiguous and
    // distinct files under different roots never collide in the set.
    let single_root = roots.len() == 1;

    for root in roots {
        for entry in ignore::WalkBuilder::new(root).require_git(false).build() {
            if cancel.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            if matches.len() >= max_results {
                return Ok(matches);
            }
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            match type_filter {
                "f" if is_dir => continue,
                "d" if !is_dir => continue,
                _ => {}
            }
            let path = entry.path();
            // Match the glob against the path relative to its own root, so a
            // pattern like '**/*.rs' behaves intuitively.
            let rel_to_root = path.strip_prefix(root).unwrap_or(path);
            if !set.is_match(rel_to_root) {
                continue;
            }
            let display = if single_root {
                rel_to_root.to_string_lossy().into_owned()
            } else {
                path.to_string_lossy().into_owned()
            };
            if !display.is_empty() {
                matches.insert(display);
            }
        }
    }
    Ok(matches)
}

/// Render the sorted match set, capping at `max_results`.
fn format_matches(matches: BTreeSet<String>, max_results: usize) -> String {
    if matches.is_empty() {
        return "(no matches)".into();
    }
    let capped = matches.len() > max_results;
    let mut lines: Vec<String> = matches.into_iter().take(max_results).collect();
    if capped {
        lines.push(format!(
            "... (capped at {max_results} results — raise max_results or narrow the pattern)"
        ));
    }
    cap_output(lines.join("\n"), ", narrow the pattern for more")
}
