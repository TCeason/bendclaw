use std::path::PathBuf;
use std::sync::Arc;

/// Build a test `SkillStore` backed by a temp directory (no DB needed for hub-only tests).
pub fn test_skill_store(
    databases: Arc<bendclaw::storage::AgentDatabases>,
    workspace_root: PathBuf,
) -> Arc<bendclaw::kernel::skills::store::SkillStore> {
    Arc::new(bendclaw::kernel::skills::store::SkillStore::new(
        databases,
        workspace_root,
        None,
    ))
}
