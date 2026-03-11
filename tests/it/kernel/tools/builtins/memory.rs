use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::agent_store::memory_store::MemoryEntry;
use bendclaw::kernel::agent_store::memory_store::MemoryResult;
use bendclaw::kernel::agent_store::memory_store::MemoryScope;
use bendclaw::kernel::agent_store::memory_store::SearchOpts;
use bendclaw::kernel::tools::memory::MemoryBackend;
use bendclaw::kernel::tools::memory::MemoryDeleteTool;
use bendclaw::kernel::tools::memory::MemoryListTool;
use bendclaw::kernel::tools::memory::MemoryReadTool;
use bendclaw::kernel::tools::memory::MemorySearchTool;
use bendclaw::kernel::tools::memory::MemoryWriteTool;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use parking_lot::Mutex;
use serde_json::json;

use crate::mocks::context::test_tool_context;

#[derive(Default)]
struct FakeMemoryBackend {
    entries: Mutex<Vec<MemoryEntry>>,
}

#[async_trait]
impl MemoryBackend for FakeMemoryBackend {
    async fn write(&self, user_id: &str, mut entry: MemoryEntry) -> bendclaw::base::Result<()> {
        entry.user_id = user_id.to_string();
        self.entries.lock().push(entry);
        Ok(())
    }

    async fn search(
        &self,
        query: &str,
        user_id: &str,
        opts: SearchOpts,
    ) -> bendclaw::base::Result<Vec<MemoryResult>> {
        let mut results: Vec<MemoryResult> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| entry.user_id == user_id)
            .filter(|entry| opts.include_shared || entry.scope != MemoryScope::Shared)
            .filter(|entry| entry.key.contains(query) || entry.content.contains(query))
            .map(|entry| MemoryResult {
                id: entry.id.clone(),
                key: entry.key.clone(),
                content: entry.content.clone(),
                scope: entry.scope,
                session_id: entry.session_id.clone(),
                score: 1.0,
                updated_at: entry.updated_at.clone(),
            })
            .collect();
        results.truncate(opts.max_results as usize);
        Ok(results)
    }

    async fn get(&self, user_id: &str, key: &str) -> bendclaw::base::Result<Option<MemoryEntry>> {
        Ok(self
            .entries
            .lock()
            .iter()
            .find(|entry| entry.user_id == user_id && entry.key == key)
            .cloned())
    }

    async fn delete(&self, user_id: &str, id: &str) -> bendclaw::base::Result<()> {
        self.entries
            .lock()
            .retain(|entry| !(entry.user_id == user_id && entry.id == id));
        Ok(())
    }

    async fn list(&self, user_id: &str, limit: u32) -> bendclaw::base::Result<Vec<MemoryEntry>> {
        let mut entries: Vec<_> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| entry.user_id == user_id)
            .cloned()
            .collect();
        entries.truncate(limit as usize);
        Ok(entries)
    }
}

fn backend() -> Arc<dyn MemoryBackend> {
    Arc::new(FakeMemoryBackend::default())
}

fn seed_entry(user_id: &str, key: &str, content: &str) -> MemoryEntry {
    MemoryEntry {
        id: "mem-1".into(),
        user_id: user_id.into(),
        scope: MemoryScope::User,
        session_id: None,
        key: key.into(),
        content: content.into(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[tokio::test]
async fn memory_write_success() -> Result<(), Box<dyn std::error::Error>> {
    let tool = MemoryWriteTool::new(backend());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"key": "test-key", "content": "test content"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("test-key"));
    Ok(())
}

#[tokio::test]
async fn memory_write_missing_fields_return_errors() -> Result<(), Box<dyn std::error::Error>> {
    let tool = MemoryWriteTool::new(backend());
    let ctx = test_tool_context();
    let missing_key = tool
        .execute_with_context(json!({"content": "test content"}), &ctx)
        .await?;
    let missing_content = tool.execute_with_context(json!({"key": "k"}), &ctx).await?;
    assert!(missing_key
        .error
        .as_deref()
        .is_some_and(|e| e.contains("key")));
    assert!(missing_content
        .error
        .as_deref()
        .is_some_and(|e| e.contains("content")));
    Ok(())
}

#[tokio::test]
async fn memory_read_and_delete_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let backend = Arc::new(FakeMemoryBackend::default());
    let ctx = test_tool_context();
    backend
        .write(&ctx.user_id, seed_entry(&ctx.user_id, "pref", "dark"))
        .await?;

    let read = MemoryReadTool::new(backend.clone())
        .execute_with_context(json!({"key": "pref"}), &ctx)
        .await?;
    assert!(read.success);
    assert!(read.output.contains("dark"));

    let delete = MemoryDeleteTool::new(backend.clone())
        .execute_with_context(json!({"id": "mem-1"}), &ctx)
        .await?;
    assert!(delete.success);

    let read_missing = MemoryReadTool::new(backend)
        .execute_with_context(json!({"key": "pref"}), &ctx)
        .await?;
    assert!(read_missing.output.contains("not found"));
    Ok(())
}

#[tokio::test]
async fn memory_search_and_list_use_fake_backend() -> Result<(), Box<dyn std::error::Error>> {
    let backend = Arc::new(FakeMemoryBackend::default());
    let ctx = test_tool_context();
    backend
        .write(
            &ctx.user_id,
            seed_entry(&ctx.user_id, "project-rust", "rust tips"),
        )
        .await?;
    backend
        .write(
            &ctx.user_id,
            seed_entry(&ctx.user_id, "project-go", "go tips"),
        )
        .await?;

    let search = MemorySearchTool::new(backend.clone())
        .execute_with_context(json!({"query": "rust", "max_results": 5}), &ctx)
        .await?;
    assert!(search.success);
    assert!(search.output.contains("project-rust"));

    let list = MemoryListTool::new(backend)
        .execute_with_context(json!({"limit": 1}), &ctx)
        .await?;
    assert!(list.success);
    assert!(list.output.contains("project-rust") || list.output.contains("project-go"));
    Ok(())
}

#[test]
fn memory_tool_metadata_is_stable() {
    let storage = backend();
    assert_eq!(MemoryWriteTool::new(storage.clone()).name(), "memory_write");
    assert_eq!(MemoryReadTool::new(storage.clone()).name(), "memory_read");
    assert_eq!(
        MemorySearchTool::new(storage.clone()).name(),
        "memory_search"
    );
    assert_eq!(
        MemoryDeleteTool::new(storage.clone()).name(),
        "memory_delete"
    );
    assert_eq!(MemoryListTool::new(storage.clone()).name(), "memory_list");

    assert_eq!(
        MemoryWriteTool::new(storage.clone()).classify_impact(&json!({})),
        None
    );
    assert_eq!(
        MemoryReadTool::new(storage.clone()).classify_impact(&json!({})),
        None
    );
    assert_eq!(
        MemorySearchTool::new(storage.clone()).classify_impact(&json!({})),
        None
    );
    assert_eq!(
        MemoryDeleteTool::new(storage.clone()).classify_impact(&json!({})),
        None
    );
    assert_eq!(
        MemoryListTool::new(storage).classify_impact(&json!({})),
        None
    );
}

#[test]
fn memory_tool_summaries_and_schemas_are_reasonable() {
    let storage = backend();
    assert_eq!(
        MemoryWriteTool::new(storage.clone()).summarize(&json!({"key": "project-x"})),
        "project-x"
    );
    assert_eq!(
        MemoryReadTool::new(storage.clone()).summarize(&json!({"key": "my-pref"})),
        "my-pref"
    );
    assert_eq!(
        MemorySearchTool::new(storage.clone()).summarize(&json!({"query": "rust tips"})),
        "rust tips"
    );
    assert_eq!(
        MemoryDeleteTool::new(storage.clone()).summarize(&json!({"id": "mem-1"})),
        "mem-1"
    );
    assert_eq!(
        MemoryListTool::new(storage.clone()).summarize(&json!({})),
        "list memories"
    );

    assert!(
        MemoryWriteTool::new(storage.clone()).parameters_schema()["properties"]["key"].is_object()
    );
    assert!(
        MemoryReadTool::new(storage.clone()).parameters_schema()["properties"]["key"].is_object()
    );
    assert!(
        MemorySearchTool::new(storage.clone()).parameters_schema()["properties"]["query"]
            .is_object()
    );
    assert!(
        MemoryDeleteTool::new(storage.clone()).parameters_schema()["properties"]["id"].is_object()
    );
    assert!(MemoryListTool::new(storage).parameters_schema()["properties"]["limit"].is_object());
}
