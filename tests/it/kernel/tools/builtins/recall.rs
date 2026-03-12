use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::tools::recall::LearningWriteBackend;
use bendclaw::kernel::tools::recall::LearningWriteTool;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::storage::dal::learning::LearningRecord;
use parking_lot::Mutex;
use serde_json::json;

use crate::mocks::context::test_tool_context;

#[derive(Default)]
struct FakeLearningBackend {
    entries: Mutex<Vec<LearningRecord>>,
}

#[async_trait]
impl LearningWriteBackend for FakeLearningBackend {
    async fn write_learning(&self, record: &LearningRecord) -> bendclaw::base::Result<()> {
        self.entries.lock().push(record.clone());
        Ok(())
    }
}

fn backend() -> Arc<FakeLearningBackend> {
    Arc::new(FakeLearningBackend::default())
}

#[tokio::test]
async fn learning_write_success() -> Result<(), Box<dyn std::error::Error>> {
    let backend = backend();
    let tool = LearningWriteTool::new(backend.clone());
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(
            json!({
                "kind": "workflow",
                "subject": "repo",
                "title": "Read AGENTS first",
                "content": "Read AGENTS.md before making repo-specific changes.",
                "priority": 7,
                "confidence": 0.9,
                "conditions": {"repo": "bendclaw"},
                "strategy": {"first_step": "read_agents"}
            }),
            &ctx,
        )
        .await?;

    assert!(result.success);
    assert!(result.output.contains("Read AGENTS first"));

    let entries = backend.entries.lock();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, "workflow");
    assert_eq!(entries[0].subject, "repo");
    assert_eq!(entries[0].priority, 7);
    assert_eq!(entries[0].confidence, 0.9);
    assert_eq!(entries[0].user_id, ctx.user_id.as_ref());
    assert!(entries[0].conditions.is_some());
    assert!(entries[0].strategy.is_some());
    Ok(())
}

#[tokio::test]
async fn learning_write_validates_required_fields() -> Result<(), Box<dyn std::error::Error>> {
    let tool = LearningWriteTool::new(backend());
    let ctx = test_tool_context();

    let missing_kind = tool
        .execute_with_context(
            json!({"subject": "repo", "title": "x", "content": "y"}),
            &ctx,
        )
        .await?;
    let missing_subject = tool
        .execute_with_context(
            json!({"kind": "workflow", "title": "x", "content": "y"}),
            &ctx,
        )
        .await?;

    assert_eq!(missing_kind.error.as_deref(), Some("kind is required"));
    assert_eq!(
        missing_subject.error.as_deref(),
        Some("subject is required")
    );
    Ok(())
}

#[tokio::test]
async fn learning_write_validates_optional_objects_and_ranges(
) -> Result<(), Box<dyn std::error::Error>> {
    let tool = LearningWriteTool::new(backend());
    let ctx = test_tool_context();

    let invalid_conditions = tool
        .execute_with_context(
            json!({
                "kind": "pattern",
                "subject": "shell",
                "title": "x",
                "content": "y",
                "conditions": ["not", "an", "object"]
            }),
            &ctx,
        )
        .await?;
    let invalid_confidence = tool
        .execute_with_context(
            json!({
                "kind": "pattern",
                "subject": "shell",
                "title": "x",
                "content": "y",
                "confidence": 1.5
            }),
            &ctx,
        )
        .await?;

    assert_eq!(
        invalid_conditions.error.as_deref(),
        Some("conditions must be an object")
    );
    assert_eq!(
        invalid_confidence.error.as_deref(),
        Some("confidence must be between 0 and 1")
    );
    Ok(())
}

#[test]
fn learning_write_metadata_is_stable() {
    let storage = backend();
    let tool = LearningWriteTool::new(storage);
    assert_eq!(tool.name(), "learning_write");
    assert_eq!(
        tool.summarize(&json!({"title": "Prefer file_edit"})),
        "Prefer file_edit"
    );
    assert_eq!(
        tool.parameters_schema()["required"],
        json!(["kind", "subject", "title", "content"])
    );
}
