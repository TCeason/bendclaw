//! Memory tools for agent memory management.

use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::agent_store::memory_store::MemoryResult;
use crate::kernel::agent_store::memory_store::SearchOpts;
use crate::kernel::agent_store::AgentStore;

mod delete;
mod list;
mod read;
mod search;
mod write;

pub use delete::MemoryDeleteTool;
pub use list::MemoryListTool;
pub use read::MemoryReadTool;
pub use search::MemorySearchTool;
pub use write::MemoryWriteTool;

#[async_trait]
pub trait MemoryBackend: Send + Sync + 'static {
    async fn write(&self, user_id: &str, entry: MemoryEntry) -> Result<()>;
    async fn search(
        &self,
        query: &str,
        user_id: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryResult>>;
    async fn get(&self, user_id: &str, key: &str) -> Result<Option<MemoryEntry>>;
    async fn delete(&self, user_id: &str, id: &str) -> Result<()>;
    async fn list(&self, user_id: &str, limit: u32) -> Result<Vec<MemoryEntry>>;
}

#[async_trait]
impl MemoryBackend for AgentStore {
    async fn write(&self, user_id: &str, entry: MemoryEntry) -> Result<()> {
        self.memory_write(user_id, entry).await
    }

    async fn search(
        &self,
        query: &str,
        user_id: &str,
        opts: SearchOpts,
    ) -> Result<Vec<MemoryResult>> {
        self.memory_search(query, user_id, opts).await
    }

    async fn get(&self, user_id: &str, key: &str) -> Result<Option<MemoryEntry>> {
        self.memory_get(user_id, key).await
    }

    async fn delete(&self, user_id: &str, id: &str) -> Result<()> {
        self.memory_delete(user_id, id).await
    }

    async fn list(&self, user_id: &str, limit: u32) -> Result<Vec<MemoryEntry>> {
        self.memory_list(user_id, limit).await
    }
}
