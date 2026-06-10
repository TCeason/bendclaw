//! Tests for tool construction — verifying tools can be built directly.

use evotengine::tools::*;

#[tokio::test]
async fn test_full_tools_complete() {
    let tools: Vec<Box<dyn evotengine::AgentTool>> = vec![
        Box::new(BashTool::default()),
        Box::new(ReadFileTool::default()),
        Box::new(WriteFileTool::new()),
        Box::new(EditFileTool::new()),
        Box::new(WebFetchTool::new()),
    ];
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 5);
    assert!(names.contains(&"read"));
    assert!(names.contains(&"edit"));
    assert!(names.contains(&"write"));
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"web_fetch"));
}

#[tokio::test]
async fn test_matches_call_name_case_insensitive() {
    use evotengine::types::AgentTool;

    let bash = BashTool::default();
    let read = ReadFileTool::default();
    let edit = EditFileTool::new();
    let write = WriteFileTool::new();

    // Base names (lowercase) always match
    assert!(bash.matches_call_name("bash"));
    assert!(read.matches_call_name("read"));
    assert!(edit.matches_call_name("edit"));
    assert!(write.matches_call_name("write"));

    // Claude-style aliases (capitalized) match via name_aliases
    assert!(bash.matches_call_name("Bash"));
    assert!(read.matches_call_name("Read"));
    assert!(edit.matches_call_name("Edit"));
    assert!(write.matches_call_name("Write"));

    // Case-insensitive: even odd casing works
    assert!(bash.matches_call_name("BASH"));
    assert!(read.matches_call_name("READ"));

    // Non-matching names don't match
    assert!(!bash.matches_call_name("read"));
    assert!(!read.matches_call_name("bash"));
}

#[tokio::test]
async fn test_readonly_tools_contains_only_safe_tools() {
    // Read-only mode now ships read plus the structured search tools.
    let tools: Vec<Box<dyn evotengine::AgentTool>> = vec![
        Box::new(ReadFileTool::default()),
        Box::new(GrepTool::new()),
        Box::new(GlobTool::new()),
    ];
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"read"));
    assert!(names.contains(&"grep"));
    assert!(names.contains(&"glob"));
    // Must not contain mutating or execution tools
    assert!(!names.contains(&"bash"));
    assert!(!names.contains(&"edit"));
    assert!(!names.contains(&"write"));
}
