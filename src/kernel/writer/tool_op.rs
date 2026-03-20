use std::sync::Arc;

use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::recall::RecallStore;
use crate::kernel::tools::builtins::memory::MemoryBackend;
use crate::kernel::writer::BackgroundWriter;
use crate::storage::dal::learning::LearningRecord;

pub enum ToolWriteOp {
    MemoryWrite {
        storage: Arc<dyn MemoryBackend>,
        user_id: String,
        entry: MemoryEntry,
    },
    LearningWrite {
        store: Arc<RecallStore>,
        record: Box<LearningRecord>,
    },
}

pub type ToolWriter = BackgroundWriter<ToolWriteOp>;

pub fn spawn_tool_writer() -> ToolWriter {
    BackgroundWriter::spawn("tool_write", 256, |op| async {
        match op {
            ToolWriteOp::MemoryWrite {
                storage,
                user_id,
                entry,
            } => {
                let key = entry.key.clone();
                if let Err(e) = storage.write(&user_id, entry).await {
                    tracing::warn!(key = %key, error = %e, "tool_writer: memory write failed");
                }
            }
            ToolWriteOp::LearningWrite { store, record } => {
                let title = record.title.clone();
                if let Err(e) = store.learnings().insert(&record).await {
                    tracing::warn!(title = %title, error = %e, "tool_writer: learning write failed");
                }
            }
        }
        true
    })
}
