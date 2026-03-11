use bendclaw::kernel::tools::web::WebSearchTool;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::mocks::context::test_tool_context;

#[tokio::test]
async fn web_search_missing_query_returns_error() -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebSearchTool;
    let ctx = test_tool_context();

    let result = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Missing or empty 'query'")));
    Ok(())
}

#[tokio::test]
async fn web_search_missing_api_key_returns_error_without_db_lookup(
) -> Result<(), Box<dyn std::error::Error>> {
    let tool = WebSearchTool;
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({"query": "databend"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("No BRAVE_API_KEY variable configured")));
    Ok(())
}
