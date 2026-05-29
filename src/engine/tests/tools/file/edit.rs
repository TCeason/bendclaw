//! Tests for EditFileTool execute and preview.

use evotengine::tools::EditFileTool;
use evotengine::types::*;

use super::super::ctx;

#[tokio::test]
async fn test_edit_file() {
    let tmp = std::env::temp_dir().join("yoagent-test-edit.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [{"old_text": "println!(\"hello\")", "new_text": "println!(\"goodbye\")"}]
            }),
            ctx("edit"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("Updated"));

    let diff = result.details["diff"].as_str().unwrap();
    assert!(diff.contains("-    println!(\"hello\")"));
    assert!(diff.contains("+    println!(\"goodbye\")"));

    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.contains("goodbye"));
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn test_edit_file_preview_command() {
    let tool = EditFileTool::new();
    let params = serde_json::json!({
        "path": "/tmp/foo.rs",
        "edits": [{"old_text": "old_code", "new_text": "new_code"}]
    });
    let cmd = tool.preview_command(&params).unwrap();
    assert!(cmd.contains("/tmp/foo.rs"));
    assert!(cmd.contains("1 replacement"));
}

#[test]
fn test_edit_file_preview_command_missing_path() {
    let tool = EditFileTool::new();
    let params = serde_json::json!({"edits": [{"old_text": "a", "new_text": "b"}]});
    assert!(tool.preview_command(&params).is_none());
}

#[tokio::test]
async fn test_edit_file_no_match() {
    let tmp = std::env::temp_dir().join("yoagent-test-edit-nomatch.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "hello world\n").unwrap();
    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": path, "edits": [{"old_text": "nonexistent", "new_text": "bar"}]}),
            ctx("edit"),
        )
        .await;
    assert!(result.is_err());
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_not_unique_error() {
    let tmp = std::env::temp_dir().join("yoagent-test-not-unique.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "aaa\nbbb\naaa\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [{"old_text": "aaa", "new_text": "ccc"}]
            }),
            ctx("edit"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("2 locations"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_empty_old_text() {
    let tmp = std::env::temp_dir().join("yoagent-test-empty-old.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "hello\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [{"old_text": "", "new_text": "bar"}]
            }),
            ctx("edit"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("old_text must not be empty"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_multi_edit() {
    let tmp = std::env::temp_dir().join("yoagent-test-multi-edit.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "fn foo() {}\nfn bar() {}\nfn baz() {}\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [
                    {"old_text": "fn foo() {}", "new_text": "fn foo_renamed() {}"},
                    {"old_text": "fn baz() {}", "new_text": "fn baz_renamed() {}"}
                ]
            }),
            ctx("edit"),
        )
        .await
        .unwrap();

    let content = std::fs::read_to_string(&tmp).unwrap();
    assert!(content.contains("fn foo_renamed() {}"));
    assert!(content.contains("fn bar() {}"));
    assert!(content.contains("fn baz_renamed() {}"));
    assert_eq!(result.details["replacement_count"], 2);
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_multi_edit_overlap_rejected() {
    let tmp = std::env::temp_dir().join("yoagent-test-overlap.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "aaa bbb ccc\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [
                    {"old_text": "aaa bbb", "new_text": "xxx"},
                    {"old_text": "bbb ccc", "new_text": "yyy"}
                ]
            }),
            ctx("edit"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("overlap"));
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_details_fields() {
    let tmp = std::env::temp_dir().join("yoagent-test-details.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [{"old_text": "println!(\"hello\")", "new_text": "println!(\"bye\")"}]
            }),
            ctx("edit"),
        )
        .await
        .unwrap();

    assert_eq!(result.details["replacement_count"], 1);
    assert!(result.details["diff"].as_str().is_some());
    let _ = std::fs::remove_file(tmp);
}

#[test]
fn test_preview_command_multi_edit() {
    let tool = EditFileTool::new();
    let params = serde_json::json!({
        "path": "/tmp/foo.rs",
        "edits": [
            {"old_text": "a", "new_text": "b"},
            {"old_text": "c", "new_text": "d"}
        ]
    });
    let cmd = tool.preview_command(&params).unwrap();
    assert!(cmd.contains("2 replacement"));
}

// ─── Matching tests ──────────────────────────────────────────────────────────

use evotengine::tools::file::edit::*;

#[test]
fn exact_unique() {
    let content = "fn main() {\n    println!(\"hello\");\n}\n";
    let old = "    println!(\"hello\");";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
    assert_eq!(m.actual_old_text, old);
}

#[test]
fn exact_not_unique() {
    let content = "aaa\nbbb\naaa\n";
    let err = resolve_unique_match(content, "aaa").unwrap_err();
    assert_eq!(err, MatchError::NotUnique { count: 2 });
}

#[test]
fn empty_old_text() {
    let err = resolve_unique_match("content", "").unwrap_err();
    assert_eq!(err, MatchError::EmptyOldText);
}

#[test]
fn unicode_normalized_match() {
    let content = "let s = \u{201C}hello\u{201D};\n";
    let old = "let s = \"hello\";";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert_eq!(m.actual_old_text, "let s = \u{201C}hello\u{201D};");
}

#[test]
fn unicode_normalized_reverse() {
    let content = "let s = \"hello\";\n";
    let old = "let s = \u{201C}hello\u{201D};";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert_eq!(m.actual_old_text, "let s = \"hello\";");
}

#[test]
fn whitespace_insensitive_match() {
    let content = "fn foo() {   \n    bar();  \n}\n";
    let old = "fn foo() {\n    bar();\n}";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "fn foo() {   \n    bar();  \n}");
}

#[test]
fn whitespace_insensitive_old_has_trailing() {
    let content = "fn foo() {\n    bar();\n}\n";
    let old = "fn foo() {  \n    bar();  \n}";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "fn foo() {\n    bar();\n}");
}

#[test]
fn not_found() {
    let content = "fn main() {}\n";
    let err = resolve_unique_match(content, "nonexistent").unwrap_err();
    assert_eq!(err, MatchError::NotFound);
}

#[test]
fn find_similar_returns_context() {
    let content = "line1\nline2\nline3\nline4\n";
    let result = find_similar_text(content, "line2");
    assert!(result.is_some());
    assert!(result.unwrap().contains("line2"));
}

#[test]
fn find_similar_empty_target() {
    assert!(find_similar_text("content", "").is_none());
}

#[test]
fn whitespace_no_trailing_newline_at_eof() {
    let content = "aaa\nbbb";
    let old = "bbb";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
    assert_eq!(m.actual_old_text, "bbb");
}

#[test]
fn whitespace_no_trailing_newline_at_eof_with_trailing_ws() {
    let content = "aaa\nbbb   ";
    let old = "bbb";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
    assert_eq!(m.actual_old_text, "bbb");
}

#[test]
fn whitespace_no_trailing_newline_ws_only_via_fallback() {
    let content = "aaa\nbbb   ";
    let old = "aaa \nbbb";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "aaa\nbbb   ");
}

#[test]
fn whitespace_match_spans_to_eof_no_newline() {
    let content = "header\nfoo()   \nbar()  ";
    let old = "foo()\nbar()";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "foo()   \nbar()  ");
}

#[test]
fn whitespace_single_line_file() {
    let content = "only_line   ";
    let old = "only_line";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::Exact);
}

#[test]
fn whitespace_single_line_file_via_fallback() {
    let content = "only_line   ";
    let old = "only_line\t";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "only_line   ");
}

#[test]
fn whitespace_old_ends_with_newline() {
    let content = "aaa\nbbb  \nccc\n";
    let old = "bbb\n";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "bbb  \n");
}

#[test]
fn whitespace_match_at_start() {
    let content = "first   \nsecond\nthird\n";
    let old = "first\nsecond";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::WhitespaceInsensitive);
    assert_eq!(m.actual_old_text, "first   \nsecond");
}

// ─── Normalize tests ─────────────────────────────────────────────────────────

#[test]
fn detect_pure_lf() {
    assert_eq!(detect_line_ending("a\nb\nc\n"), LineEnding::Lf);
}

#[test]
fn detect_pure_crlf() {
    assert_eq!(detect_line_ending("a\r\nb\r\nc\r\n"), LineEnding::CrLf);
}

#[test]
fn detect_mixed_majority_crlf() {
    assert_eq!(detect_line_ending("a\r\nb\r\nc\n"), LineEnding::CrLf);
}

#[test]
fn detect_empty_defaults_lf() {
    assert_eq!(detect_line_ending(""), LineEnding::Lf);
}

#[test]
fn normalize_crlf_to_lf() {
    assert_eq!(normalize_to_lf("a\r\nb\r\n"), "a\nb\n");
}

#[test]
fn normalize_bare_cr() {
    assert_eq!(normalize_to_lf("a\rb\r"), "a\nb\n");
}

#[test]
fn restore_to_crlf() {
    assert_eq!(
        restore_line_endings("a\nb\n", LineEnding::CrLf),
        "a\r\nb\r\n"
    );
}

#[test]
fn restore_to_lf_noop() {
    assert_eq!(restore_line_endings("a\nb\n", LineEnding::Lf), "a\nb\n");
}

#[test]
fn strip_bom_present() {
    let input = "\u{FEFF}hello";
    let (bom, content) = strip_utf8_bom(input);
    assert_eq!(bom, "\u{FEFF}");
    assert_eq!(content, "hello");
}

#[test]
fn strip_bom_absent() {
    let (bom, content) = strip_utf8_bom("hello");
    assert_eq!(bom, "");
    assert_eq!(content, "hello");
}

#[test]
fn normalize_curly_quotes() {
    let input = "\u{201C}hello\u{201D} \u{2018}world\u{2019}";
    let result = normalize_unicode(input);
    assert_eq!(result, "\"hello\" 'world'");
    assert_eq!(input.chars().count(), result.chars().count());
}

#[test]
fn normalize_unicode_no_change() {
    let input = "\"hello\" 'world'";
    assert_eq!(normalize_unicode(input), input);
}

#[test]
fn preserve_quote_style_no_normalization() {
    let result = preserve_quote_style("hello", "hello", "world");
    assert_eq!(result, "world");
}

#[test]
fn preserve_quote_style_double_curly() {
    let old = "say \"hello\"";
    let actual = "say \u{201C}hello\u{201D}";
    let new = "say \"goodbye\"";
    let result = preserve_quote_style(old, actual, new);
    assert_eq!(result, "say \u{201C}goodbye\u{201D}");
}

#[test]
fn preserve_quote_style_single_curly() {
    let old = "it's a 'test'";
    let actual = "it\u{2019}s a \u{2018}test\u{2019}";
    let new = "it's a 'demo'";
    let result = preserve_quote_style(old, actual, new);
    assert_eq!(result, "it\u{2019}s a \u{2018}demo\u{2019}");
}

#[test]
fn preserve_quote_style_mixed() {
    let old = "\"hello\" and 'world'";
    let actual = "\u{201C}hello\u{201D} and \u{2018}world\u{2019}";
    let new = "\"goodbye\" and 'earth'";
    let result = preserve_quote_style(old, actual, new);
    assert_eq!(result, "\u{201C}goodbye\u{201D} and \u{2018}earth\u{2019}");
}

#[test]
fn preserve_quote_style_no_curly_in_actual() {
    let result = preserve_quote_style("abc", "def", "ghi");
    assert_eq!(result, "ghi");
}

// ─── Unicode dash / NBSP normalization tests ─────────────────────────────────

#[test]
fn unicode_dash_normalized_match() {
    // em-dash in file, hyphen in old_text
    let content = "// This is a long \u{2014} explanation\n";
    let old = "// This is a long - explanation";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert!(m.actual_old_text.contains('\u{2014}'));
}

#[test]
fn unicode_en_dash_normalized_match() {
    let content = "pages 10\u{2013}20\n";
    let old = "pages 10-20";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert!(m.actual_old_text.contains('\u{2013}'));
}

#[test]
fn unicode_minus_sign_normalized_match() {
    let content = "x \u{2212} y\n";
    let old = "x - y";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert!(m.actual_old_text.contains('\u{2212}'));
}

#[test]
fn nbsp_normalized_match() {
    // NBSP in file, regular space in old_text
    let content = "let\u{00A0}x = 1;\n";
    let old = "let x = 1;";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert!(m.actual_old_text.contains('\u{00A0}'));
}

#[test]
fn normalize_unicode_dashes() {
    let input = "a\u{2010}b\u{2013}c\u{2014}d\u{2212}e";
    assert_eq!(normalize_unicode(input), "a-b-c-d-e");
    assert_eq!(
        input.chars().count(),
        normalize_unicode(input).chars().count()
    );
}

#[test]
fn normalize_unicode_nbsp() {
    let input = "hello\u{00A0}world";
    assert_eq!(normalize_unicode(input), "hello world");
}

// ─── Full normalization (Level 4) tests ──────────────────────────────────────

#[test]
fn full_normalized_quotes_plus_trailing_ws() {
    // curly quotes + trailing whitespace — Level 2 handles this because
    // the substring search finds the match within the line.
    // Level 4 is needed when old_text itself has trailing ws that differs.
    let content = "let s = \u{201C}hello\u{201D};   \n";
    let old = "let s = \"hello\";";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert_eq!(m.actual_old_text, "let s = \u{201C}hello\u{201D};");
}

#[test]
fn full_normalized_dash_plus_trailing_ws() {
    // Same: Level 2 substring search handles dash normalization
    let content = "// long \u{2014} text   \nfn foo() {}\n";
    let old = "// long - text";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::UnicodeNormalized);
    assert!(m.actual_old_text.contains('\u{2014}'));
}

#[test]
fn full_normalized_quotes_and_ws_in_old_text() {
    // old_text has trailing ws that doesn't match file — Level 2 fails,
    // Level 3 fails (quotes differ), Level 4 catches it.
    let content = "let s = \u{201C}hello\u{201D};\n";
    let old = "let s = \"hello\";  ";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::FullNormalized);
}

#[test]
fn full_normalized_multiline_quotes_and_ws() {
    // Multiline: curly quotes + trailing ws on each line
    let content = "let a = \u{201C}x\u{201D};   \nlet b = \u{2014};  \n";
    let old = "let a = \"x\";  \nlet b = -;";
    let m = resolve_unique_match(content, old).unwrap();
    assert_eq!(m.kind, MatchKind::FullNormalized);
    assert!(m.actual_old_text.contains('\u{201C}'));
    assert!(m.actual_old_text.contains('\u{2014}'));
}

// ─── Overlap error message tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_overlap_error_includes_indices() {
    let tmp = std::env::temp_dir().join("evot-test-overlap-idx.txt");
    let path = tmp.to_str().unwrap();
    std::fs::write(&tmp, "aaa bbb ccc\n").unwrap();

    let tool = EditFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({
                "path": path,
                "edits": [
                    {"old_text": "aaa bbb", "new_text": "xxx"},
                    {"old_text": "bbb ccc", "new_text": "yyy"}
                ]
            }),
            ctx("edit"),
        )
        .await;

    let err = result.unwrap_err().to_string();
    assert!(err.contains("edits[0]"));
    assert!(err.contains("edits[1]"));
    assert!(err.contains("overlap"));
    let _ = std::fs::remove_file(tmp);
}
