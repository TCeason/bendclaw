//! Tests for [`SkillCreateTool`].

use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use bendclaw::kernel::tools::skill::SkillCreateTool;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::mocks::context::test_tool_context;
use crate::mocks::skill::test_skill_store;

fn dummy_databases() -> Arc<bendclaw::storage::AgentDatabases> {
    let pool =
        bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool");
    Arc::new(bendclaw::storage::AgentDatabases::new(pool, "test_").unwrap())
}

fn make_tool() -> SkillCreateTool {
    let databases = dummy_databases();
    let factory = Arc::new(DatabendSkillRepositoryFactory::new(databases.clone()));
    let dir = std::env::temp_dir().join(format!("bendclaw-create-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let store = test_skill_store(databases, dir);
    SkillCreateTool::new(factory, store)
}

fn valid_args() -> serde_json::Value {
    json!({
        "name": "json-to-csv",
        "description": "Convert JSON to CSV",
        "content": "## Parameters\n- `--input` : Path to JSON file (required)",
        "script_name": "run.py",
        "script_body": "import json, sys\nprint('ok')"
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Name validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_path_traversal_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("../evil");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_uppercase_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("MySkill");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_reserved_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("shell");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_empty_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// File path validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_md_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.md");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("extension")));
    Ok(())
}

#[tokio::test]
async fn create_rejects_rb_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.rb");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_js_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.js");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Size validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_oversized_content() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["content"] = json!("x".repeat(10 * 1024 + 1));
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("content exceeds")));
    Ok(())
}

#[tokio::test]
async fn create_rejects_oversized_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_body"] = json!("x".repeat(50 * 1024 + 1));
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("exceeds")));
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// summarize
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn summarize_returns_name() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = make_tool();
    assert_eq!(tool.summarize(&json!({"name": "my-skill"})), "my-skill");
}

#[test]
fn summarize_returns_unknown_when_name_missing() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = make_tool();
    assert_eq!(tool.summarize(&json!({})), "unknown");
}

// ── Tool trait metadata ──

#[test]
fn create_tool_name() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    assert_eq!(tool.name(), "create_skill");
}

#[test]
fn create_tool_description() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn create_tool_schema_has_required_fields() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["name"].is_object());
    assert!(schema["properties"]["description"].is_object());
    assert!(schema["properties"]["content"].is_object());
    assert!(schema["properties"]["script_name"].is_object());
    assert!(schema["properties"]["script_body"].is_object());
}

#[test]
fn create_tool_op_type() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    let tool = make_tool();
    assert_eq!(tool.op_type(), OpType::SkillRun);
}
