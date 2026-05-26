//! Tests for EditFileTool execute and preview.

use evotengine::tools::edit::EditFileTool;
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
            ctx("Edit"),
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
            ctx("Edit"),
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
            ctx("Edit"),
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
            ctx("Edit"),
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
            ctx("Edit"),
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
            ctx("Edit"),
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
            ctx("Edit"),
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
