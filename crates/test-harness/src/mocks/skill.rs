use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::skills::remote::repository::SkillRepository;
use bendclaw::kernel::skills::skill::Skill;
use parking_lot::Mutex;

/// Skill store that does nothing (for tests that don't need persistence).
pub struct NoopSkillStore;

#[async_trait]
impl SkillRepository for NoopSkillStore {
    async fn list(&self) -> bendclaw::base::Result<Vec<Skill>> {
        Ok(vec![])
    }
    async fn get(&self, _name: &str) -> bendclaw::base::Result<Option<Skill>> {
        Ok(None)
    }
    async fn save(&self, _skill: &Skill) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn remove(&self, _name: &str, _agent_id: Option<&str>) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn checksums(&self) -> bendclaw::base::Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }
}

/// In-memory [`SkillRepository`] for unit-testing tools that create/remove skills.
pub struct MockSkillStore {
    skills: Mutex<HashMap<String, Skill>>,
}

impl MockSkillStore {
    pub fn new() -> Self {
        Self {
            skills: Mutex::new(HashMap::new()),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.skills.lock().contains_key(name)
    }

    pub fn get_skill(&self, name: &str) -> Option<Skill> {
        self.skills.lock().get(name).cloned()
    }
}

#[async_trait]
impl SkillRepository for MockSkillStore {
    async fn list(&self) -> bendclaw::base::Result<Vec<Skill>> {
        Ok(self.skills.lock().values().cloned().collect())
    }

    async fn get(&self, name: &str) -> bendclaw::base::Result<Option<Skill>> {
        Ok(self.skills.lock().get(name).cloned())
    }

    async fn save(&self, skill: &Skill) -> bendclaw::base::Result<()> {
        self.skills.lock().insert(skill.name.clone(), skill.clone());
        Ok(())
    }

    async fn remove(&self, name: &str, _agent_id: Option<&str>) -> bendclaw::base::Result<()> {
        self.skills.lock().remove(name);
        Ok(())
    }

    async fn checksums(&self) -> bendclaw::base::Result<HashMap<String, String>> {
        let map = self
            .skills
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), v.compute_sha256()))
            .collect();
        Ok(map)
    }
}

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
