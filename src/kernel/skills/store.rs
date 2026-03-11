//! SkillStore — filesystem-backed skill catalog. No in-memory cache.
//!
//! Architecture:
//!   DB  →  local `.remote/` mirror  (via periodic sync)
//!   Hub →  local `.hub/` directory  (via git clone/pull)
//!
//! All queries read from the local filesystem.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::base::Result;
use crate::config::HubConfig;
use crate::kernel::skills::fs::load_skill_tree;
use crate::kernel::skills::fs::load_skills;
use crate::kernel::skills::fs::LoadedSkill;
use crate::kernel::skills::hub;
use crate::kernel::skills::remote;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillScope;
use crate::kernel::skills::skill::SkillSource;
use crate::storage::AgentDatabases;

pub struct SkillStore {
    databases: Arc<AgentDatabases>,
    workspace_root: PathBuf,
    hub_config: Option<HubConfig>,
}

impl SkillStore {
    pub fn new(
        databases: Arc<AgentDatabases>,
        workspace_root: PathBuf,
        hub_config: Option<HubConfig>,
    ) -> Self {
        let _ = std::fs::create_dir_all(hub::paths::hub_dir(&workspace_root));
        Self {
            databases,
            workspace_root,
            hub_config,
        }
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────

    /// Sync hub repo and pull remote skills from DB to local mirror.
    pub async fn refresh(&self) -> Result<()> {
        self.ensure_hub();
        self.sync_remote_skills().await
    }

    /// Ensure the hub git repo is cloned / up-to-date.
    fn ensure_hub(&self) {
        let Some(hub_cfg) = &self.hub_config else {
            return;
        };
        hub::sync::ensure(
            &self.workspace_root,
            &hub_cfg.repo_url,
            hub_cfg.sync_interval_secs,
        );
    }

    pub async fn sync_remote_skills(&self) -> Result<()> {
        remote::sync::sync(&self.databases, &self.workspace_root).await
    }

    // ── Query ─────────────────────────────────────────────────────────────

    /// Return all skills visible to the given agent, deduplicated by name.
    /// Agent-scoped skills override hub skills with the same name.
    pub fn for_agent(&self, agent_id: &str) -> Vec<Skill> {
        use std::collections::HashMap;
        let mut by_name: HashMap<String, Skill> = HashMap::new();
        // Hub skills first (lower priority)
        for loaded in self.load_hub_skills() {
            by_name.insert(loaded.skill.name.clone(), loaded.skill);
        }
        // Agent-scoped remote skills override hub by name
        let remote_dir = remote::paths::remote_dir(&self.workspace_root, agent_id);
        for loaded in Self::load_from_dir(&remote_dir) {
            by_name.insert(loaded.skill.name.clone(), loaded.skill);
        }
        let mut skills: Vec<Skill> = by_name.into_values().collect();
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills
    }

    /// Look up a single skill by name: agent-scoped first, then hub/global.
    pub fn get(&self, agent_id: &str, name: &str) -> Option<Skill> {
        self.resolve_loaded(agent_id, name).map(|l| l.skill)
    }

    pub fn get_hub(&self, name: &str) -> Option<Skill> {
        self.resolve_hub_loaded(name).map(|l| l.skill)
    }

    /// Resolve a skill (with namespace support) and return the full LoadedSkill.
    pub fn resolve(&self, agent_id: &str, tool_name: &str) -> Option<Skill> {
        self.resolve_loaded(agent_id, tool_name)
            .map(|l| l.skill)
            .or_else(|| {
                // Handle namespace/skill-name format
                tool_name
                    .find('/')
                    .and_then(|idx| self.resolve_loaded(agent_id, &tool_name[idx + 1..]))
                    .map(|l| l.skill)
            })
    }

    pub fn script_path(&self, agent_id: &str, tool_name: &str) -> Option<String> {
        let loaded = self.resolve_loaded_with_ns(agent_id, tool_name)?;
        let host_script = loaded.script_path()?;
        let hub_dir = hub::paths::hub_dir(&self.workspace_root);
        if let Ok(rel) = host_script.strip_prefix(&hub_dir) {
            return Some(format!("/workspace/skills/.hub/{}", rel.to_string_lossy()));
        }
        let agents_dir = self.workspace_root.join("agents");
        if let Ok(rel) = host_script.strip_prefix(&agents_dir) {
            return Some(format!("/workspace/agents/{}", rel.to_string_lossy()));
        }
        None
    }

    pub fn host_script_path(&self, agent_id: &str, tool_name: &str) -> Option<PathBuf> {
        self.resolve_loaded_with_ns(agent_id, tool_name)
            .and_then(|l| l.script_path())
    }

    pub fn read_skill(&self, agent_id: &str, path: &str) -> Option<String> {
        if path.contains("..") {
            return None;
        }
        // Try exact match
        if let Some(loaded) = self.resolve_loaded(agent_id, path) {
            return Some(loaded.skill.content);
        }
        // Try prefix match: "skill-name/sub/path"
        for (idx, _) in path.match_indices('/').rev() {
            if let Some(loaded) = self.resolve_loaded(agent_id, &path[..idx]) {
                return loaded.read_doc(&path[idx + 1..]);
            }
        }
        None
    }

    // ── Write ─────────────────────────────────────────────────────────────

    pub fn insert(&self, skill: &Skill, agent_id: &str) {
        remote::writer::write_skill(&self.workspace_root, agent_id, skill);
    }

    pub fn evict(&self, name: &str, agent_id: &str) {
        remote::writer::remove_skill(&self.workspace_root, agent_id, name);
    }

    // ── Accessors ─────────────────────────────────────────────────────────

    pub fn loaded_skills(&self) -> Vec<LoadedSkill> {
        let mut all = self.load_hub_skills();
        // Scan all agent remote dirs
        let agents_dir = self.workspace_root.join("agents");
        if let Ok(entries) = std::fs::read_dir(&agents_dir) {
            for entry in entries.flatten() {
                let agent_dir = entry.path();
                if !agent_dir.is_dir() {
                    continue;
                }
                let remote_dir = agent_dir.join("skills").join(".remote");
                all.extend(Self::load_from_dir(&remote_dir));
            }
        }
        all
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn hub_config(&self) -> Option<&HubConfig> {
        self.hub_config.as_ref()
    }

    // ── Internal ──────────────────────────────────────────────────────────

    fn load_hub_skills(&self) -> Vec<LoadedSkill> {
        let hub_dir = hub::paths::hub_dir(&self.workspace_root);
        let mut skills = load_skills(&hub_dir);
        for loaded in &mut skills {
            loaded.skill.source = SkillSource::Hub;
            loaded.skill.scope = SkillScope::Global;
        }
        skills
    }

    /// Load all skills from a directory (each subdirectory is a skill).
    fn load_from_dir(dir: &Path) -> Vec<LoadedSkill> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut skills = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) if !n.starts_with('_') && !n.starts_with('.') => n.to_string(),
                _ => continue,
            };
            if let Some(loaded) = load_skill_tree(&path, &dir_name) {
                skills.push(loaded);
            }
        }
        skills
    }

    /// Resolve a single skill by name: agent remote dir first, then hub.
    fn resolve_loaded(&self, agent_id: &str, name: &str) -> Option<LoadedSkill> {
        // Agent-scoped remote skill
        let remote_skill_dir = remote::paths::skill_dir(&self.workspace_root, agent_id, name);
        if remote_skill_dir.join("SKILL.md").exists() {
            return load_skill_tree(&remote_skill_dir, name);
        }
        self.resolve_hub_loaded(name)
    }

    /// Resolve with namespace/skill-name fallback.
    fn resolve_loaded_with_ns(&self, agent_id: &str, tool_name: &str) -> Option<LoadedSkill> {
        self.resolve_loaded(agent_id, tool_name).or_else(|| {
            tool_name
                .find('/')
                .and_then(|idx| self.resolve_loaded(agent_id, &tool_name[idx + 1..]))
        })
    }

    fn resolve_hub_loaded(&self, name: &str) -> Option<LoadedSkill> {
        let hub_skill_dir = hub::paths::hub_dir(&self.workspace_root).join(name);
        let mut loaded = load_skill_tree(&hub_skill_dir, name)?;
        loaded.skill.source = SkillSource::Hub;
        loaded.skill.scope = SkillScope::Global;
        Some(loaded)
    }
}
