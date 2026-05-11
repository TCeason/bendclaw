//! Tests for ListFilesTool.

use evotengine::tools::list::ListFilesTool;
use evotengine::types::*;

use super::ctx;

#[tokio::test]
async fn test_list_files_tool() -> Result<(), ToolError> {
    let tmp_dir = std::env::temp_dir().join("yoagent-test-list2");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("sub")).map_err(|e| ToolError::Failed(e.to_string()))?;
    std::fs::write(tmp_dir.join("a.rs"), "").map_err(|e| ToolError::Failed(e.to_string()))?;
    std::fs::write(tmp_dir.join("sub/c.rs"), "").map_err(|e| ToolError::Failed(e.to_string()))?;
    let tool = ListFilesTool::new();
    let path = tmp_dir
        .to_str()
        .ok_or_else(|| ToolError::Failed("invalid temp path".into()))?;
    let result = tool
        .execute(serde_json::json!({"path": path}), ctx("list_files"))
        .await?;
    let text = if let Content::Text { text } = &result.content[0] {
        text
    } else {
        return Err(ToolError::Failed("expected text content".into()));
    };
    assert!(text.contains("a.rs"));
    assert_eq!(result.details["slimmed"], false);
    let _ = std::fs::remove_dir_all(tmp_dir);
    Ok(())
}

#[tokio::test]
async fn test_list_files_slims_large_results() -> Result<(), ToolError> {
    let tmp_dir = std::env::temp_dir().join("yoagent-test-list-slim");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("src")).map_err(|e| ToolError::Failed(e.to_string()))?;
    std::fs::create_dir_all(tmp_dir.join("tests")).map_err(|e| ToolError::Failed(e.to_string()))?;

    for i in 0..90 {
        let dir = if i % 2 == 0 { "src" } else { "tests" };
        std::fs::write(tmp_dir.join(dir).join(format!("file_{i}.rs")), "")
            .map_err(|e| ToolError::Failed(e.to_string()))?;
    }

    let tool = ListFilesTool::new();
    let path = tmp_dir
        .to_str()
        .ok_or_else(|| ToolError::Failed("invalid temp path".into()))?;
    let result = tool
        .execute(serde_json::json!({"path": path}), ctx("list_files"))
        .await?;

    let text = if let Content::Text { text } = &result.content[0] {
        text
    } else {
        return Err(ToolError::Failed("expected text content".into()));
    };

    assert!(text.contains("[90 files; compact grouped view]"));
    assert!(text.contains("src (45)"));
    assert!(text.contains("tests (45)"));
    assert_eq!(result.details["slimmed"], true);

    let _ = std::fs::remove_dir_all(tmp_dir);
    Ok(())
}
