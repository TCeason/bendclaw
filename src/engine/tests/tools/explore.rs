//! Tests for the explore tools: grep (content search) and glob (file search).
//!
//! These exercise the in-process fallback path (gitignore-aware walk) and the
//! shared dispatch. When rg/fd are on PATH the external path is used instead;
//! both backends are required to produce equivalent, relativized output.

use std::sync::Arc;

use evotengine::tools::GlobTool;
use evotengine::tools::GrepTool;
use evotengine::tools::SearchTool;
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
        supports_image: true,
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

#[tokio::test]
async fn grep_context_includes_surrounding_lines() {
    let dir = fixture();
    let tool = GrepTool::new();
    // main.rs: line 1 `fn main() {`, line 2 `println!("hello")`, line 3 `}`.
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "println", "context": 1, "reason": "context" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    // The match line uses ':' and the context lines use '-' (ripgrep style).
    assert!(out.contains("src/main.rs:2: "), "match line missing: {out}");
    assert!(
        out.contains("src/main.rs-1- "),
        "before-context missing: {out}"
    );
    assert!(
        out.contains("src/main.rs-3- "),
        "after-context missing: {out}"
    );
}

#[tokio::test]
async fn grep_fixed_strings_matches_literally() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("f.txt"), "a.b.c\naxbxc\n").unwrap();
    let tool = GrepTool::new();
    // As a regex `a.b.c` would also match `axbxc`; fixed_strings must not.
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": "a.b.c",
                "fixed_strings": true,
                "reason": "literal"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    assert!(
        out.contains("f.txt:1: a.b.c"),
        "literal match missing: {out}"
    );
    assert!(!out.contains("axbxc"), "regex-style match leaked: {out}");
}

#[tokio::test]
async fn grep_files_with_matches_lists_paths_only() {
    let dir = fixture();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": "fn ",
                "files_with_matches": true,
                "reason": "list files"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    // Only bare paths, no line numbers or match text.
    assert!(out.contains("src/main.rs"), "path missing: {out}");
    assert!(out.contains("src/lib.rs"), "path missing: {out}");
    assert!(!out.contains("src/main.rs:"), "line detail leaked: {out}");
    // Each matching file should appear exactly once.
    assert_eq!(
        out.matches("src/main.rs").count(),
        1,
        "path duplicated: {out}"
    );
}

// GLOB_TESTS

#[tokio::test]
async fn grep_gitignore_false_searches_ignored_files() {
    let dir = fixture();
    let tool = GrepTool::new();
    // The fixture writes target/generated.rs (gitignored) containing
    // `hello_generated`. Default search hides it; gitignore:false surfaces it.
    let hidden = tool
        .execute(
            serde_json::json!({ "pattern": "hello_generated", "reason": "default" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    assert_eq!(
        text_of(&hidden),
        "(no matches)",
        "ignored file leaked by default"
    );

    let shown = tool
        .execute(
            serde_json::json!({
                "pattern": "hello_generated",
                "gitignore": false,
                "reason": "search ignored"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    assert!(
        text_of(&shown).contains("generated.rs:1:"),
        "ignored file not surfaced with gitignore=false: {}",
        text_of(&shown)
    );
}

#[tokio::test]
async fn grep_include_accepts_array() {
    let dir = fixture();
    let tool = GrepTool::new();
    // 'hello' appears in README.md and src/main.rs; restrict to both globs.
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": "hello",
                "include": ["*.md", "*.rs"],
                "reason": "union of globs"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    assert!(out.contains("README.md:"), "md missing: {out}");
    assert!(out.contains("src/main.rs:"), "rs missing: {out}");
}

#[tokio::test]
async fn grep_multiline_matches_across_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), "start\nfoo\nbar\nend\n").unwrap();
    let tool = GrepTool::new();
    // `(?s)foo.*bar` only matches when '.' crosses newlines (multiline on).
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": "(?s)foo.*bar",
                "multiline": true,
                "reason": "cross-line"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    assert!(out.contains("a.txt:"), "multiline match missing: {out}");
}

#[tokio::test]
async fn grep_skip_paginates() {
    let dir = tempfile::tempdir().expect("tempdir");
    // One file, three matching lines.
    std::fs::write(dir.path().join("f.txt"), "hit 1\nhit 2\nhit 3\n").unwrap();
    let tool = GrepTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": "hit", "skip": 2, "reason": "paginate" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("grep ok");
    let out = text_of(&res);
    // First two matches skipped; only line 3 remains.
    assert!(out.contains("f.txt:3:"), "expected third match: {out}");
    assert!(!out.contains("f.txt:1:"), "first match not skipped: {out}");
    assert!(!out.contains("f.txt:2:"), "second match not skipped: {out}");
}

#[tokio::test]
async fn glob_gitignore_false_finds_ignored() {
    let dir = fixture();
    let tool = GlobTool::new();
    // target/generated.rs is gitignored; default hides it, gitignore:false shows.
    let res = tool
        .execute(
            serde_json::json!({
                "pattern": ["**/*.rs"],
                "gitignore": false,
                "reason": "include ignored"
            }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    assert!(
        text_of(&res).contains("generated.rs"),
        "ignored file not surfaced: {}",
        text_of(&res)
    );
}

#[tokio::test]
async fn glob_hidden_toggle() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".secret.txt"), "x").unwrap();
    std::fs::write(dir.path().join("plain.txt"), "y").unwrap();
    let tool = GlobTool::new();

    let without = tool
        .execute(
            serde_json::json!({ "pattern": ["*.txt"], "reason": "default" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    assert!(
        !text_of(&without).contains(".secret.txt"),
        "hidden leaked by default"
    );

    let with = tool
        .execute(
            serde_json::json!({ "pattern": ["*.txt"], "hidden": true, "reason": "show hidden" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    assert!(
        text_of(&with).contains(".secret.txt"),
        "hidden file missing with hidden=true: {}",
        text_of(&with)
    );
}

#[tokio::test]
async fn glob_sorts_by_mtime_newest_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Write old first, then new after a real delay, so new.rs has a later mtime.
    std::fs::write(dir.path().join("old.rs"), "a").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dir.path().join("new.rs"), "b").unwrap();
    let tool = GlobTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "pattern": ["*.rs"], "reason": "recency" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("glob ok");
    let out = text_of(&res);
    let new_pos = out.find("new.rs").expect("new.rs present");
    let old_pos = out.find("old.rs").expect("old.rs present");
    assert!(new_pos < old_pos, "newest should sort first: {out}");
}

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

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

/// A richer fixture with named definitions across files.
fn search_fixture() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("src/auth.rs"),
        "/// Authenticate a user against the credential store.\n\
         pub fn authenticate_user(name: &str, password: &str) -> bool {\n\
             let hash = hash_password(password);\n\
             verify_credentials(name, &hash)\n\
         }\n\
         \n\
         fn hash_password(p: &str) -> String {\n\
             format!(\"hashed:{p}\")\n\
         }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/config.rs"),
        "/// Parse a TOML config file from disk.\n\
         pub fn parse_config(path: &str) -> Config {\n\
             let text = std::fs::read_to_string(path).unwrap();\n\
             toml::from_str(&text).unwrap()\n\
         }\n",
    )
    .unwrap();
    // A test file referencing auth — should be down-ranked vs the real impl.
    std::fs::write(
        root.join("tests/auth_test.rs"),
        "#[test]\n\
         fn test_authenticate_user_works() {\n\
             assert!(authenticate_user(\"a\", \"b\"));\n\
             assert!(authenticate_user(\"c\", \"d\"));\n\
         }\n",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn search_finds_definition_by_concept() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "query": "authenticate user" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("search ok");
    let out = text_of(&res);
    assert!(out.contains("src/auth.rs"), "got: {out}");
    assert!(out.contains("defines `authenticate_user`"), "got: {out}");
}

#[tokio::test]
async fn search_ranks_impl_above_test() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "query": "authenticate user", "top_k": 5 }),
            ctx_at(dir.path()),
        )
        .await
        .expect("search ok");
    let out = text_of(&res);
    let impl_pos = out.find("src/auth.rs").expect("impl present");
    // The noise penalty should keep the real impl ahead of the test file.
    if let Some(test_pos) = out.find("auth_test.rs") {
        assert!(impl_pos < test_pos, "impl should rank first; got: {out}");
    }
}

#[tokio::test]
async fn search_matches_identifier_stems() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    // "parse config" should surface `parse_config` via stem matching.
    let res = tool
        .execute(
            serde_json::json!({ "query": "parse config" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("search ok");
    let out = text_of(&res);
    assert!(out.contains("src/config.rs"), "got: {out}");
    assert!(out.contains("defines `parse_config`"), "got: {out}");
}

#[tokio::test]
async fn search_no_matches_reports_cleanly() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let res = tool
        .execute(
            serde_json::json!({ "query": "zzzznonexistenttoken" }),
            ctx_at(dir.path()),
        )
        .await
        .expect("search ok");
    assert_eq!(text_of(&res), "(no matches)");
}

#[tokio::test]
async fn search_missing_query_errors() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let err = tool
        .execute(serde_json::json!({}), ctx_at(dir.path()))
        .await
        .expect_err("should error");
    assert!(matches!(err, ToolError::InvalidArgs(_)));
}

#[tokio::test]
async fn search_second_call_uses_cache() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    // First call builds + caches the index; second reuses it. Both must succeed
    // and return the same top result for a deterministic query.
    let q = serde_json::json!({ "query": "parse config" });
    let a = tool
        .execute(q.clone(), ctx_at(dir.path()))
        .await
        .expect("a");
    let b = tool.execute(q, ctx_at(dir.path())).await.expect("b");
    assert_eq!(text_of(&a), text_of(&b));
}

#[tokio::test]
async fn search_reflects_file_modification() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let cx = || ctx_at(dir.path());

    // Prime the cache.
    let _ = tool
        .execute(serde_json::json!({ "query": "parse config" }), cx())
        .await
        .expect("initial search");

    // A brand-new symbol is not yet indexed.
    let before = text_of(
        &tool
            .execute(serde_json::json!({ "query": "reticulate splines" }), cx())
            .await
            .expect("search before edit"),
    );
    assert!(
        !before.contains("reticulate_splines"),
        "symbol should not exist yet: {before}"
    );

    // Modify an existing file to add the symbol (size changes -> detected).
    std::fs::write(
        dir.path().join("src/config.rs"),
        "/// Parse a TOML config file from disk.\n\
         pub fn parse_config(path: &str) -> Config {\n\
             let text = std::fs::read_to_string(path).unwrap();\n\
             toml::from_str(&text).unwrap()\n\
         }\n\
         \n\
         pub fn reticulate_splines(count: usize) -> usize {\n\
             count * 2\n\
         }\n",
    )
    .unwrap();

    let after = text_of(
        &tool
            .execute(serde_json::json!({ "query": "reticulate splines" }), cx())
            .await
            .expect("search after edit"),
    );
    assert!(
        after.contains("defines `reticulate_splines`"),
        "modified file should be re-indexed: {after}"
    );
}

#[tokio::test]
async fn search_reflects_new_file() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let cx = || ctx_at(dir.path());

    let _ = tool
        .execute(serde_json::json!({ "query": "authenticate user" }), cx())
        .await
        .expect("prime cache");

    // Add a new source file after the index was built.
    std::fs::write(
        dir.path().join("src/payments.rs"),
        "/// Charge a customer card via the payment gateway.\n\
         pub fn charge_payment(amount: u64) -> bool {\n\
             amount > 0\n\
         }\n",
    )
    .unwrap();

    let out = text_of(
        &tool
            .execute(serde_json::json!({ "query": "charge payment" }), cx())
            .await
            .expect("search after add"),
    );
    assert!(
        out.contains("defines `charge_payment`"),
        "new file should be indexed: {out}"
    );
}

#[tokio::test]
async fn search_reflects_deleted_file() {
    let dir = search_fixture();
    let tool = SearchTool::new();
    let cx = || ctx_at(dir.path());

    let before = text_of(
        &tool
            .execute(serde_json::json!({ "query": "parse config" }), cx())
            .await
            .expect("prime cache"),
    );
    assert!(before.contains("src/config.rs"), "config present: {before}");

    std::fs::remove_file(dir.path().join("src/config.rs")).unwrap();

    let after = text_of(
        &tool
            .execute(serde_json::json!({ "query": "parse config" }), cx())
            .await
            .expect("search after delete"),
    );
    assert!(
        !after.contains("src/config.rs"),
        "deleted file must drop out of the index: {after}"
    );
}

#[test]
fn search_name_resolves_and_aliases_match() {
    let tool = SearchTool::new();
    // Canonical snake_case name is used everywhere except Claude.
    assert_eq!(tool.name(), "semantic_code_search");
    assert_eq!(tool.resolve_name("gpt-4o"), "semantic_code_search");
    // On Claude it presents the PascalCase name, matching the sibling explore
    // tools (Read, Grep, Glob) so the tool list reads consistently.
    assert_eq!(tool.resolve_name("claude-opus-4-6"), "SemanticCodeSearch");
    // Both the canonical name and the Claude alias route back to this tool.
    assert!(tool.matches_call_name("semantic_code_search"));
    assert!(tool.matches_call_name("SemanticCodeSearch"));
    assert!(!tool.matches_call_name("grep"));
}

/// Real-world smoke test against a large local checkout. Ignored by default
/// (path-specific); run with:
///   cargo test -p evotengine --test tools explore::search_databend_smoke -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn search_databend_smoke() {
    let root =
        std::path::PathBuf::from(std::env::var("HOME").unwrap() + "/github/databendlabs/databend");
    if !root.exists() {
        eprintln!("skip: {} not found", root.display());
        return;
    }
    let tool = SearchTool::new();

    let t0 = std::time::Instant::now();
    let res = tool
        .execute(
            serde_json::json!({ "query": "compaction segment merge", "top_k": 5 }),
            ctx_at(&root),
        )
        .await
        .expect("search ok");
    let cold = t0.elapsed();

    let t1 = std::time::Instant::now();
    let _ = tool
        .execute(
            serde_json::json!({ "query": "http request handler", "top_k": 5 }),
            ctx_at(&root),
        )
        .await
        .expect("search ok");
    let warm = t1.elapsed();

    eprintln!(
        "cold (build+query): {cold:?}\nwarm (query only): {warm:?}\n---\n{}",
        text_of(&res)
    );
    assert!(text_of(&res).contains("compact"), "expected compaction hit");
    // Warm query reuses the cache and should be far faster than the cold build.
    assert!(warm < cold, "warm query should reuse cached index");
}
