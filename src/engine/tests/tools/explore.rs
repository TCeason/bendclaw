//! Tests for the explore tools: grep (content search) and glob (file search).
//!
//! These exercise the in-process fallback path (gitignore-aware walk) and the
//! shared dispatch. When rg/fd are on PATH the external path is used instead;
//! both backends are required to produce equivalent, relativized output.

use std::sync::Arc;

use evotengine::tools::GlobTool;
use evotengine::tools::GrepTool;
use evotengine::types::*;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// Build a ToolContext rooted at `dir`.
fn ctx_at(dir: &std::path::Path) -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: "explore".into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
        cwd: dir.to_path_buf(),
        path_guard: Arc::new(evotengine::PathGuard::open()),
        spill: None,
    }
}

/// Create a small project tree with a .gitignore for fallback-path testing.
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "pub fn greet() -> &'static str {\n    \"hi\"\n}\n",
    )
    .unwrap();
    std::fs::write(root.join("README.md"), "# Title\nhello world\n").unwrap();
    // Should be ignored by .gitignore in both backends.
    std::fs::write(
        root.join("target/generated.rs"),
        "fn hello_generated() {}\n",
    )
    .unwrap();
    dir
}

fn text_of(result: &ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// GREP_TESTS

#[tokio::test]
async fn grep_returns_path_line_text() {
    let dir = fixture();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "fn ", "reason": "find functions" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    // Must include line numbers in path:line: text form.
    assert!(out.contains("src/main.rs:1:"), "got: {out}");
    assert!(out.contains("src/lib.rs:1:"), "got: {out}");
}

#[tokio::test]
async fn grep_respects_gitignore() {
    let dir = fixture();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "hello", "reason": "check ignore" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    assert!(out.contains("main.rs"), "got: {out}");
    // The gitignored target/ file must not appear.
    assert!(!out.contains("generated.rs"), "ignored file leaked: {out}");
}

#[tokio::test]
async fn grep_include_filter() {
    let dir = fixture();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": "hello",
                "include": "*.md",
                "reason": "only markdown"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    assert!(out.contains("README.md"), "got: {out}");
    assert!(!out.contains("main.rs"), "include filter ignored: {out}");
}

#[tokio::test]
async fn grep_ignore_case() {
    let dir = fixture();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": "HELLO",
                "ignore_case": true,
                "reason": "case insensitive"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    assert!(text_of(&res).contains("hello"), "case-insensitive failed");
}

#[tokio::test]
async fn grep_skips_binary_files() {
    let dir = fixture();
    // A file with a NUL byte is detected as binary by the search engine and
    // must not produce matches, even though the token is present as bytes.
    std::fs::write(dir.path().join("blob.bin"), b"hello\x00\x00binary").unwrap();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "hello", "reason": "binary check" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    assert!(!out.contains("blob.bin"), "binary file matched: {out}");
}

#[tokio::test]
async fn grep_no_matches() {
    let dir = fixture();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "zzz_no_such_token", "reason": "x" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    assert_eq!(text_of(&res), "(no matches)");
}

#[tokio::test]
async fn grep_missing_pattern_errors() {
    let dir = fixture();
    let tool = GrepTool::new();
    let err = tool
        .execute(serde_json::json!({ "reason": "x" }), ctx_at(dir.path()))
        .await
        .expect_err("should error");
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

// GLOB_TESTS

#[tokio::test]
async fn glob_finds_by_pattern() {
    let dir = fixture();
    let tool = GlobTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": ["**/*.rs"], "reason": "all rust files" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    let out = text_of(&res);
    assert!(out.contains("src/main.rs"), "got: {out}");
    assert!(out.contains("src/lib.rs"), "got: {out}");
}

#[tokio::test]
async fn glob_respects_gitignore() {
    let dir = fixture();
    let tool = GlobTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": ["**/*.rs"], "reason": "check ignore" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    let out = text_of(&res);
    assert!(!out.contains("generated.rs"), "ignored file leaked: {out}");
}

#[tokio::test]
async fn glob_unions_multiple_patterns() {
    let dir = fixture();
    let tool = GlobTool::new();
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": ["**/*.md", "src/**/*.rs"],
                "reason": "union of two patterns"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    let out = text_of(&res);
    assert!(out.contains("README.md"), "got: {out}");
    assert!(out.contains("src/main.rs"), "got: {out}");
}

#[tokio::test]
async fn glob_accepts_bare_string() {
    let dir = fixture();
    let tool = GlobTool::new();
    // A scalar string is coerced to a single-element pattern list.
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "**/*.md", "reason": "scalar form" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    assert!(text_of(&res).contains("README.md"));
}

#[tokio::test]
async fn glob_type_directory() {
    let dir = fixture();
    let tool = GlobTool::new();
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": ["**"],
                "type": "d",
                "reason": "directories only"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    let out = text_of(&res);
    assert!(out.contains("src"), "expected src dir: {out}");
    // A file should not appear under type=d.
    assert!(!out.contains("main.rs"), "file leaked under type=d: {out}");
}

#[tokio::test]
async fn glob_no_matches() {
    let dir = fixture();
    let tool = GlobTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": ["**/*.nonexistent"], "reason": "x" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    assert_eq!(text_of(&res), "(no matches)");
}

#[tokio::test]
async fn glob_missing_pattern_errors() {
    let dir = fixture();
    let tool = GlobTool::new();
    let err = tool
        .execute(serde_json::json!({ "reason": "x" }), ctx_at(dir.path()))
        .await
        .expect_err("should error");
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}
