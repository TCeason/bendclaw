use bendclaw::cli::repl::diff::diff_from_details;
use bendclaw::cli::repl::diff::format_diff;

#[test]
fn no_changes_shows_message() {
    let result = format_diff("hello\n", "hello\n");
    assert_eq!(result.lines_added, 0);
    assert_eq!(result.lines_removed, 0);
    assert!(result.text.contains("no changes"));
}

#[test]
fn single_line_insert() {
    let result = format_diff("a\nb\n", "a\nx\nb\n");
    assert_eq!(result.lines_added, 1);
    assert_eq!(result.lines_removed, 0);
    assert!(result.text.contains("+x"));
}

#[test]
fn single_line_delete() {
    let result = format_diff("a\nb\nc\n", "a\nc\n");
    assert_eq!(result.lines_added, 0);
    assert_eq!(result.lines_removed, 1);
    assert!(result.text.contains("-b"));
}

#[test]
fn replace_line() {
    let result = format_diff("a\nold\nc\n", "a\nnew\nc\n");
    assert_eq!(result.lines_added, 1);
    assert_eq!(result.lines_removed, 1);
    assert!(result.text.contains("-old"));
    assert!(result.text.contains("+new"));
}

#[test]
fn diff_from_details_with_old_and_new() {
    let details = serde_json::json!({
        "old_content": "line1\nline2\n",
        "new_content": "line1\nchanged\n",
    });
    let diff = diff_from_details(&details).unwrap();
    assert!(diff.contains("-line2"));
    assert!(diff.contains("+changed"));
}

#[test]
fn diff_from_details_new_file() {
    let details = serde_json::json!({
        "new_content": "hello\nworld\n",
    });
    let diff = diff_from_details(&details).unwrap();
    assert!(diff.contains("+hello"));
    assert!(diff.contains("+world"));
}

#[test]
fn diff_from_details_no_content_returns_none() {
    let details = serde_json::json!({ "path": "/tmp/foo" });
    assert!(diff_from_details(&details).is_none());
}

#[test]
fn diff_from_details_identical_returns_none() {
    let details = serde_json::json!({
        "old_content": "same\n",
        "new_content": "same\n",
    });
    assert!(diff_from_details(&details).is_none());
}
