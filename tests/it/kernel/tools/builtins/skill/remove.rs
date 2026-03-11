//! Tests for [`SkillRemoveTool`].

use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use bendclaw::kernel::tools::skill::SkillRemoveTool;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::mocks::context::test_tool_context;
use crate::mocks::skill::test_skill_store;

fn dummy_databases() -> Arc<bendclaw::storage::AgentDatabases> {
    let pool =
        bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool");
    Arc::new(bendclaw::storage::AgentDatabases::new(pool, "test_").unwrap())
}

fn make_tool() -> SkillRemoveTool {
    let databases = dummy_databases();
    let factory = Arc::new(DatabendSkillRepositoryFactory::new(databases.clone()));
    let dir = std::env::temp_dir().join(format!("bendclaw-rm-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let store = test_skill_store(databases, dir);
    SkillRemoveTool::new(factory, store)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Validation errors (these don't hit the store, so they work with dummy pools)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn remove_rejects_path_traversal_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "../evil"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("skill name")));
    Ok(())
}

#[tokio::test]
async fn remove_rejects_empty_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({"name": ""}), &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_single_char_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "a"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_uppercase_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "MySkill"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_reserved_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "shell"}), &ctx)
        .await?;
    assert!(!result.success);
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
fn remove_tool_name() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    assert_eq!(tool.name(), "remove_skill");
}

#[test]
fn remove_tool_description() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn remove_tool_schema_has_name_field() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["name"].is_object());
}

#[test]
fn remove_tool_op_type() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    let tool = make_tool();
    assert_eq!(tool.op_type(), OpType::SkillRun);
}
