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
         Use `context` to include surrounding lines (like grep -C) so you can read a match in \
         place without a follow-up Read. Use `files_with_matches` to list only the matching file \
         paths, and `fixed_strings` to match the pattern literally instead of as a regex. \
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
                "context": {
                    "type": "number",
                    "description": "Number of context lines to show before and after each match (like grep -C). Context lines use 'path-line- text' (hyphen) to distinguish them from match lines. Defaults to 0."
                },
                "fixed_strings": {
                    "type": "boolean",
                    "description": "Treat the pattern as a literal string instead of a regex (like grep -F). Useful for searching text with regex metacharacters. Defaults to false."
                },
                "files_with_matches": {
                    "type": "boolean",
                    "description": "List only the paths of files containing a match, one per line (like grep -l), instead of the matching lines. Defaults to false."
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
        let mut flags = String::from("-rn");
        if params["ignore_case"].as_bool().unwrap_or(false) {
            flags.push('i');
        }
        if params["fixed_strings"].as_bool().unwrap_or(false) {
            flags.push('F');
        }
        if params["files_with_matches"].as_bool().unwrap_or(false) {
            flags.push('l');
        }
        let context = match params["context"].as_u64() {
            Some(n) if n > 0 => format!(" -C {n}"),
            _ => String::new(),
        };
        Some(format!("grep {flags}{context} {pattern:?} {path}"))
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
        let fixed_strings = params["fixed_strings"].as_bool().unwrap_or(false);
        let files_with_matches = params["files_with_matches"].as_bool().unwrap_or(false);
        let context = params["context"].as_u64().unwrap_or(0) as usize;

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
            SearchOptions {
                ignore_case,
                fixed_strings,
                files_with_matches,
                context,
            },
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
use grep::searcher::BinaryDetection;
use grep::searcher::Searcher;
use grep::searcher::SearcherBuilder;
use grep::searcher::Sink;
use grep::searcher::SinkContext;
use grep::searcher::SinkContextKind;
use grep::searcher::SinkMatch;

/// Options controlling a single grep run.
#[derive(Clone, Copy)]
struct SearchOptions {
    ignore_case: bool,
    fixed_strings: bool,
    files_with_matches: bool,
    context: usize,
}

/// Gitignore-aware walk + ripgrep's own search engine, run off the async
/// runtime. Using grep-searcher gives binary-file detection, encoding
/// handling, and line counting for free — the same machinery ripgrep uses.
async fn run_search(
    root: &Path,
    pattern: &str,
    include: Option<&str>,
    opts: SearchOptions,
    ctx: &ToolContext,
) -> Result<String, ToolError> {
    let matcher = grep::regex::RegexMatcherBuilder::new()
        .case_insensitive(opts.ignore_case)
        .fixed_strings(opts.fixed_strings)
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
    tokio::task::spawn_blocking(move || search_tree(&root, &matcher, glob.as_ref(), opts, &cancel))
        .await
        .map_err(|e| ToolError::Failed(format!("search task panicked: {e}")))?
}

fn search_tree(
    root: &Path,
    matcher: &RegexMatcher,
    glob: Option<&globset::GlobMatcher>,
    opts: SearchOptions,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<String, ToolError> {
    // -l mode reports paths only, so context lines are irrelevant there.
    let context = if opts.files_with_matches {
        0
    } else {
        opts.context
    };
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .before_context(context)
        .after_context(context)
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

        if opts.files_with_matches {
            // -l: report the path once on the first match, then move on.
            if lines.len() >= MAX_MATCHES {
                capped = true;
                break 'outer;
            }
            let mut matched = false;
            let sink = FilesWithMatchesSink {
                matched: &mut matched,
            };
            let _ = searcher.search_path(matcher, path, sink);
            if matched {
                lines.push(rel);
            }
            continue;
        }

        let mut hit_cap = false;
        let sink = GrepSink {
            rel: &rel,
            lines: &mut lines,
            hit_cap: &mut hit_cap,
        };
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

/// Sink that emits `path:line: text` for matches and `path-line- text` for
/// context lines (mirroring ripgrep's match/context separators), capping the
/// total number of emitted lines at `MAX_MATCHES`.
struct GrepSink<'a> {
    rel: &'a str,
    lines: &'a mut Vec<String>,
    hit_cap: &'a mut bool,
}

impl GrepSink<'_> {
    fn push(&mut self, sep: char, line_no: Option<u64>, bytes: &[u8]) -> bool {
        if self.lines.len() >= MAX_MATCHES {
            *self.hit_cap = true;
            return false;
        }
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim_end();
        match line_no {
            Some(n) => self.lines.push(format!("{}{sep}{n}{sep} {text}", self.rel)),
            None => self.lines.push(format!("{}{sep} {text}", self.rel)),
        }
        true
    }
}

impl Sink for GrepSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch) -> Result<bool, std::io::Error> {
        Ok(self.push(':', mat.line_number(), mat.bytes()))
    }

    fn context(&mut self, _searcher: &Searcher, ctx: &SinkContext) -> Result<bool, std::io::Error> {
        let sep = match ctx.kind() {
            SinkContextKind::Before | SinkContextKind::After => '-',
            SinkContextKind::Other => '-',
        };
        Ok(self.push(sep, ctx.line_number(), ctx.bytes()))
    }
}

/// Sink for `-l` mode: flips a flag on the first match and stops the file scan.
struct FilesWithMatchesSink<'a> {
    matched: &'a mut bool,
}

impl Sink for FilesWithMatchesSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, _mat: &SinkMatch) -> Result<bool, std::io::Error> {
        *self.matched = true;
        Ok(false) // one match is enough; stop scanning this file
    }
}
