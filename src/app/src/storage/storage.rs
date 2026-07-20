use async_trait::async_trait;

use crate::error::Result;
use crate::search::SessionWithText;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::VariableRecord;

#[async_trait]
pub trait Storage: Send + Sync {
    async fn save_session(&self, session: SessionMeta) -> Result<()>;
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>>;
    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>>;
    async fn list_sessions_with_text(&self, limit: usize) -> Result<Vec<SessionWithText>>;

    async fn delete_session(&self, session_id: &str) -> Result<bool>;

    async fn append_entry(&self, entry: TranscriptEntry) -> Result<()> {
        self.append_entries(vec![entry]).await
    }
    /// Append one logical transcript batch. Implementations must preserve the
    /// order of entries and avoid interleaving another batch within it.
    async fn append_entries(&self, entries: Vec<TranscriptEntry>) -> Result<()>;
    /// Atomically append a batch only when the persisted transcript still ends
    /// at `expected_seq`. Returns `false` when another writer won the race.
    async fn compare_and_append_entries(
        &self,
        expected_seq: u64,
        entries: Vec<TranscriptEntry>,
    ) -> Result<bool>;
    async fn list_entries(&self, params: ListTranscriptEntries) -> Result<Vec<TranscriptEntry>>;

    async fn load_variables(&self) -> Result<Vec<VariableRecord>>;
    async fn save_variables(&self, variables: Vec<VariableRecord>) -> Result<()>;

    /// Session ids the user pinned as favorites in the dashboard. Stored
    /// independently of session metadata so toggling never rewrites a session.
    async fn load_favorites(&self) -> Result<Vec<String>>;
    async fn save_favorites(&self, ids: Vec<String>) -> Result<()>;
}
