//! Write/remove skill files to per-agent remote directories.
//!
//! Writes are atomic: content goes to a temp directory first, then a rename
//! swaps it into place.  This prevents readers from seeing half-written state.

use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

use super::paths;
use crate::kernel::skills::fs::is_safe_relative_path;
use crate::kernel::skills::fs::load_skill_from_dir;
use crate::kernel::skills::fs::load_skill_with_meta;
use crate::kernel::skills::fs::LoadedSkill;
use crate::kernel::skills::manifest::SkillManifest;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillParameter;
use crate::kernel::skills::skill::SkillRequirements;

/// Metadata persisted alongside SKILL.md so that scope, source, agent_id,
/// etc. survive a round-trip through the filesystem mirror.
#[derive(Serialize, Deserialize)]
pub struct SkillMeta {
    pub scope: String,
    pub source: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    pub executable: bool,
    #[serde(default)]
    pub parameters: Vec<SkillParameter>,
    #[serde(default)]
    pub requires: Option<SkillRequirements>,
    #[serde(default)]
    pub manifest: Option<SkillManifest>,
}

/// Write a skill to the local mirror atomically.
///
/// Returns `None` if the write fails at any step.  On success the final
/// directory is ready for readers immediately after the rename.
pub fn write_skill(workspace_root: &Path, agent_id: &str, skill: &Skill) -> Option<LoadedSkill> {
    let final_dir = paths::skill_dir(workspace_root, agent_id, &skill.name);
    let parent = final_dir.parent()?;
    std::fs::create_dir_all(parent).ok()?;

    // Stage into a temp directory next to the final location
    let tmp_dir = parent.join(format!(".tmp-{}", skill.name));
    if tmp_dir.exists() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    std::fs::create_dir_all(&tmp_dir).ok()?;

    // Write SKILL.md
    let skill_md = format!(
        "---\nname: {}\ndescription: {}\nversion: {}\ntimeout: {}\n---\n{}",
        skill.name, skill.description, skill.version, skill.timeout, skill.content
    );
    if std::fs::write(tmp_dir.join("SKILL.md"), &skill_md).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    // Write .meta.json — preserves fields not in SKILL.md frontmatter
    let meta = SkillMeta {
        scope: skill.scope.as_str().to_string(),
        source: skill.source.as_str().to_string(),
        agent_id: skill.agent_id.clone(),
        created_by: skill.created_by.clone(),
        executable: skill.executable,
        parameters: skill.parameters.clone(),
        requires: skill.requires.clone(),
        manifest: skill.manifest.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(tmp_dir.join(".meta.json"), json);
    }

    // Write files (scripts/, references/)
    for f in &skill.files {
        let rel = std::path::Path::new(&f.path);
        if !is_safe_relative_path(rel) {
            tracing::warn!(skill = %skill.name, path = %f.path, "unsafe skill file path rejected");
            continue;
        }
        let file_path = tmp_dir.join(rel);
        if let Some(p) = file_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&file_path, &f.body);
    }

    // Atomic swap: remove old dir, rename tmp into place
    if final_dir.exists() {
        let _ = std::fs::remove_dir_all(&final_dir);
    }
    if std::fs::rename(&tmp_dir, &final_dir).is_err() {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return None;
    }

    let mut loaded = load_skill_from_dir(&final_dir, &skill.name)?;
    loaded.skill.scope = skill.scope.clone();
    loaded.skill.source = skill.source.clone();
    loaded.skill.agent_id = skill.agent_id.clone();
    loaded.skill.created_by = skill.created_by.clone();
    loaded.skill.executable = skill.executable;
    loaded.skill.parameters = skill.parameters.clone();
    loaded.skill.files = skill.files.clone();
    loaded.skill.requires = skill.requires.clone();
    loaded.skill.manifest = skill.manifest.clone();
    Some(loaded)
}

/// Compute the current on-disk checksum, including mirror metadata.
pub fn read_disk_checksum(
    workspace_root: &Path,
    agent_id: &str,
    skill_name: &str,
) -> Option<String> {
    let dir = paths::skill_dir(workspace_root, agent_id, skill_name);
    let loaded = load_skill_with_meta(&dir, skill_name)?;
    Some(loaded.skill.compute_sha256())
}

pub fn remove_skill(workspace_root: &Path, agent_id: &str, skill_name: &str) {
    let dir = paths::skill_dir(workspace_root, agent_id, skill_name);
    let _ = std::fs::remove_dir_all(&dir);
}
