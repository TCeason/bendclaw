//! Test helpers for building Session.

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(feature = "live-tests")]
use anyhow::Result;
#[cfg(feature = "live-tests")]
use bendclaw::kernel::agent_store::AgentStore;
#[cfg(feature = "live-tests")]
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
#[cfg(feature = "live-tests")]
use bendclaw::kernel::session::Session;
#[cfg(feature = "live-tests")]
use bendclaw::kernel::session::SessionResources;
#[cfg(feature = "live-tests")]
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
#[cfg(feature = "live-tests")]
use bendclaw::kernel::tools::registry::create_session_tools;
use bendclaw::kernel::tools::ToolContext;
#[cfg(feature = "live-tests")]
use bendclaw::llm::provider::LLMProvider;
use bendclaw::storage::Pool;
#[cfg(feature = "live-tests")]
use parking_lot::RwLock;

#[cfg(feature = "live-tests")]
use crate::mocks::skill::test_skill_store;

/// Build a test Workspace for a temp directory.
pub fn test_workspace(dir: std::path::PathBuf) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        std::time::Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

/// Create a dummy Pool that points to a non-existent endpoint.
/// Suitable for tests that never actually query the database.
pub fn dummy_pool() -> Pool {
    Pool::new("http://localhost:0", "", "default").expect("dummy pool: invalid URL is unreachable")
}

/// Build a test `Session` with tools wired up.
pub fn test_tool_context() -> ToolContext {
    use ulid::Ulid;
    let dir = std::env::temp_dir().join(format!("bendclaw-test-ctx-{}", Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    ToolContext {
        user_id: format!("u-{}", Ulid::new()).into(),
        session_id: format!("s-{}", Ulid::new()).into(),
        agent_id: "a1".into(),
        workspace: test_workspace(dir),
        pool: dummy_pool(),
    }
}

#[cfg(feature = "live-tests")]
pub async fn test_session(llm: Arc<dyn LLMProvider>) -> Result<Session> {
    let config = Arc::new(AgentConfig::default());

    let pool = crate::common::setup::pool().await?;

    let databases =
        Arc::new(bendclaw::storage::AgentDatabases::new(pool.clone(), "test_").unwrap());

    let workspace_dir = std::env::temp_dir().join("bendclaw-test-session");
    let _ = std::fs::create_dir_all(&workspace_dir);

    let skills = test_skill_store(databases.clone(), workspace_dir.clone());

    let storage = Arc::new(AgentStore::new(pool.clone(), llm.clone()));

    let workspace = test_workspace(workspace_dir);

    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    let skill_store_factory = Arc::new(DatabendSkillRepositoryFactory::new(databases));
    let tool_registry = Arc::new(create_session_tools(
        storage.clone(),
        skills.clone(),
        skill_store_factory,
        pool.clone(),
        channels,
        "test_instance".to_string(),
    ));

    let tools = Arc::new(tool_registry.tool_schemas());

    Ok(Session::new(
        "s1".to_string(),
        "a1".into(),
        "u1".into(),
        SessionResources {
            workspace,
            tool_registry,
            skills,
            tools,
            storage,
            llm: Arc::new(RwLock::new(llm)),
            config,
            variables: vec![],
        },
    ))
}
