//! Tests for GlobFileTool.

use evotengine::tools::glob_file::GlobFileTool;
use evotengine::types::*;

use super::ctx;

#[tokio::test]
async fn test_glob_file_tool() -> Result<(), ToolError> {
    let tmp_dir = std::env::temp_dir().join("yoagent-test-glob-file");
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(tmp_dir.join("sub")).map_err(|e| ToolError::Failed(e.to_string()))?;
    std::fs::write(tmp_dir.join("a.rs"), "").map_err(|e| ToolError::Failed(e.to_string()))?;
    std::fs::write(tmp_dir.join("sub/c.rs"), "").map_err(|e| ToolError::Failed(e.to_string()))?;
    std::fs::write(tmp_dir.join("sub/c.txt"), "").map_err(|e| ToolError::Failed(e.to_string()))?;

    let tool = GlobFileTool::new();
    let path = tmp_dir
        .to_str()
        .ok_or_else(|| ToolError::Failed("invalid temp path".into()))?;
    let result = tool
        .execute(
            serde_json::json!({"pattern": "**/*.rs", "path": path}),
            ctx("Glob"),
        )
        .await?;

    let text = if let Content::Text { text } = &result.content[0] {
        text
    } else {
        return Err(ToolError::Failed("expected text content".into()));
    };
    assert!(text.contains("a.rs"));
    assert!(text.contains("sub/c.rs"));
    assert!(!text.contains("sub/c.txt"));
    assert_eq!(result.details["files"], 2);

    let _ = std::fs::remove_dir_all(tmp_dir);
    Ok(())
}
