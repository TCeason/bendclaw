//! Tests for tool construction — verifying tools can be built directly.

use evotengine::tools::*;

#[tokio::test]
async fn test_full_tools_complete() {
    let tools: Vec<Box<dyn evotengine::AgentTool>> = vec![
        Box::new(BashTool::default()),
        Box::new(ReadFileTool::default()),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::new()),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
        Box::new(WebFetchTool::new()),
    ];
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"edit_file"));
    assert!(names.contains(&"list_files"));
    assert!(names.contains(&"web_fetch"));
}

#[tokio::test]
async fn test_readonly_tools_contains_only_safe_tools() {
    let tools: Vec<Box<dyn evotengine::AgentTool>> = vec![
        Box::new(ReadFileTool::default()),
        Box::new(ListFilesTool::default()),
        Box::new(SearchTool::default()),
    ];
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"list_files"));
    assert!(names.contains(&"search"));
    // Must not contain mutating or execution tools
    assert!(!names.contains(&"bash"));
    assert!(!names.contains(&"edit_file"));
    assert!(!names.contains(&"write_file"));
    assert!(!names.contains(&"web_fetch"));
}
