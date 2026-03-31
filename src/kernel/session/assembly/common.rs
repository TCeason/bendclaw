use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::session::workspace::OpenResolver;
use crate::kernel::session::workspace::SandboxResolver;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::variables::Variable;
use crate::llm::tool::ToolSchema;

/// Build workspace from config + variables. Shared by both assemblers.
pub fn build_workspace(
    config: &AgentConfig,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
    cwd_override: Option<&Path>,
    variables: &[Variable],
) -> crate::base::Result<Arc<Workspace>> {
    let workspace_dir = config.workspace.session_dir(user_id, agent_id, session_id);
    if let Err(e) = std::fs::create_dir_all(&workspace_dir) {
        return Err(crate::base::ErrorCode::internal(format!(
            "failed to create session workspace: {e}"
        )));
    }

    let cwd = cwd_override.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        if config.workspace.sandbox {
            workspace_dir.clone()
        } else {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| workspace_dir.clone())
        }
    });

    let resolver: Arc<dyn crate::kernel::session::workspace::PathResolver> =
        if config.workspace.sandbox {
            Arc::new(SandboxResolver)
        } else {
            Arc::new(OpenResolver)
        };

    Ok(Arc::new(Workspace::from_variables(
        workspace_dir,
        cwd,
        config.workspace.safe_env_vars.clone(),
        variables,
        Duration::from_secs(config.workspace.command_timeout_secs),
        Duration::from_secs(config.workspace.max_command_timeout_secs),
        config.workspace.max_output_bytes,
        resolver,
    )))
}

/// Build workspace for ephemeral sessions (no agent/user, minimal config).
pub fn build_workspace_ephemeral(
    config: &AgentConfig,
    session_id: &str,
    cwd_override: Option<&Path>,
) -> crate::base::Result<Arc<Workspace>> {
    build_workspace(config, "agent", session_id, "cli", cwd_override, &[])
}

/// Apply tool filter to schemas. Returns the allowed_tool_names set if a filter was given.
pub fn apply_tool_filter(
    tools: &mut Vec<ToolSchema>,
    filter: Option<HashSet<String>>,
) -> Option<HashSet<String>> {
    filter.map(|f| {
        tools.retain(|t| f.contains(&t.function.name));
        tools.iter().map(|t| t.function.name.clone()).collect()
    })
}
