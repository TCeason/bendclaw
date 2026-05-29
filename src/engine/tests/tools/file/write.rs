//! Tests for WriteFileTool.

use evotengine::tools::*;
use evotengine::types::*;

use super::super::ctx;

#[tokio::test]
async fn test_read_write_file() {
    let tmp = std::env::temp_dir().join("yoagent-test-rw.txt");
    let path = tmp.to_str().unwrap();

    // Write
    let write_tool = WriteFileTool::new();
    let result = write_tool
        .execute(
            serde_json::json!({"path": path, "content": "hello from yoagent"}),
            ctx("write"),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("Wrote"));

    // Read
    let read_tool = ReadFileTool::new();
    let result = read_tool
        .execute(serde_json::json!({"path": path}), ctx("read"))
        .await
        .unwrap();

    let text = match &result.content[0] {
        Content::Text { text } => text,
        _ => panic!("expected text"),
    };
    assert!(text.contains("hello from yoagent"));

    // Cleanup
    let _ = std::fs::remove_file(tmp);
}

#[tokio::test]
async fn test_write_creates_directories() {
    let tmp = std::env::temp_dir().join("yoagent-test-nested/deep/dir/file.txt");
    let path = tmp.to_str().unwrap();

    let tool = WriteFileTool::new();
    let result = tool
        .execute(
            serde_json::json!({"path": path, "content": "nested!"}),
            ctx("write"),
        )
        .await;

    assert!(result.is_ok());
    assert!(tmp.exists());

    // Cleanup
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("yoagent-test-nested"));
}
