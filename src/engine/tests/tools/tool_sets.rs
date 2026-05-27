//! Tests for tool construction — verifying tools can be built directly.

use evotengine::tools::*;

#[tokio::test]
async fn test_full_tools_complete() {
    let tools: Vec<Box<dyn evotengine::AgentTool>> = vec![
        Box::new(BashTool::default()),
        Box::new(ReadFileTool::default()),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::new()),
        Box::new(GlobFileTool::default()),
        Box::new(SearchTool::default()),
        Box::new(WebFetchTool::new()),
    ];
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"read"));
    assert!(names.contains(&"edit"));
    assert!(names.contains(&"Glob"));
    assert!(names.contains(&"WebFetch"));
}

#[tokio::test]
async fn test_readonly_tools_contains_only_safe_tools() {
    let tools: Vec<Box<dyn evotengine::AgentTool>> = vec![
        Box::new(ReadFileTool::default()),
        Box::new(GlobFileTool::default()),
        Box::new(SearchTool::default()),
    ];
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"read"));
    assert!(names.contains(&"Glob"));
    assert!(names.contains(&"Grep"));
    // Must not contain mutating or execution tools
    assert!(!names.contains(&"bash"));
    assert!(!names.contains(&"edit"));
    assert!(!names.contains(&"write"));
    assert!(!names.contains(&"WebFetch"));
}
