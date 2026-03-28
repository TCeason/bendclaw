use std::sync::Arc;
use std::time::Duration;

use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use parking_lot::RwLock;

fn make_session(id: &str) -> Arc<Session> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let dir = std::env::temp_dir().join(format!("bendclaw-clear-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let workspace = Arc::new(Workspace::new(
        dir.clone(),
        dir.clone(),
        vec!["PATH".into(), "HOME".into()],
        std::collections::HashMap::new(),
        Duration::from_secs(5),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ));
    let pool = bendclaw_test_harness::mocks::context::dummy_pool();
    let databases =
        Arc::new(bendclaw::storage::AgentDatabases::new(pool.clone(), "unit_").unwrap());
    let skills = Arc::new(SkillStore::new(databases, dir, None));
    Arc::new(Session::new(
        id.into(),
        "a1".into(),
        "u1".into(),
        SessionResources {
            workspace,
            tool_registry: Arc::new(ToolRegistry::new()),
            skills,
            tools: Arc::new(vec![]),
            storage: Arc::new(AgentStore::new(pool, llm.clone())),
            llm: Arc::new(RwLock::new(llm)),
            config: Arc::new(AgentConfig::default()),
            prompt_variables: vec![],
            cluster_client: None,
            directive: None,
            trace_writer: bendclaw::kernel::trace::TraceWriter::noop(),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            prompt_config: None,
            memory: None,
        },
    ))
}

#[test]
fn clear_history_does_not_panic_on_empty_session() {
    let session = make_session("s1");
    session.clear_history();
    assert!(session.is_idle());
}

#[test]
fn clear_history_preserves_session_state() {
    let session = make_session("s1");
    session.clear_history();
    assert!(session.is_idle());
    assert!(!session.is_running());
    assert!(!session.is_stale());
    assert!(session.belongs_to("a1", "u1"));
}
