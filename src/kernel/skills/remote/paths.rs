//! Per-agent remote skill directory paths.

use std::path::Path;
use std::path::PathBuf;

/// `{workspace_root}/agents/{agent_id}/skills/.remote/`
pub fn remote_dir(workspace_root: &Path, agent_id: &str) -> PathBuf {
    workspace_root
        .join("agents")
        .join(agent_id)
        .join("skills")
        .join(".remote")
}

/// `{workspace_root}/agents/{agent_id}/skills/.remote/{skill_name}/`
pub fn skill_dir(workspace_root: &Path, agent_id: &str, skill_name: &str) -> PathBuf {
    remote_dir(workspace_root, agent_id).join(skill_name)
}
