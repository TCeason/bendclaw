use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::skills::remote::repository::DatabendSkillRepositoryFactory;
use bendclaw::kernel::tools::registry::create_session_tools;
use bendclaw::llm::provider::LLMProvider;
use parking_lot::RwLock;

use crate::mocks::skill::test_skill_store;

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

pub async fn test_session(llm: Arc<dyn LLMProvider>) -> Result<Session> {
    let config = Arc::new(AgentConfig::default());
    let pool = crate::common::setup::pool().await?;
    let databases = Arc::new(bendclaw::storage::AgentDatabases::new(
        pool.clone(),
        "test_",
    )?);

    let workspace_dir =
        std::env::temp_dir().join(format!("bendclaw-live-session-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&workspace_dir)?;

    let skills = test_skill_store(databases.clone(), workspace_dir.clone());
    let storage = Arc::new(AgentStore::new(pool.clone(), llm.clone()));
    let workspace = test_workspace(workspace_dir);
    let channels = Arc::new(bendclaw::kernel::channel::registry::ChannelRegistry::new());
    let skill_store_factory = Arc::new(DatabendSkillRepositoryFactory::new(databases));
    let tool_registry = Arc::new(create_session_tools(
        storage.clone(),
        skills.clone(),
        skill_store_factory,
        pool,
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
