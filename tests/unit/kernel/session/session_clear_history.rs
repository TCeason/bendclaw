use std::sync::Arc;
use std::time::Duration;

use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::session::Session;
use bendclaw::kernel::session::SessionResources;
use bendclaw::kernel::skills::projector::SkillProjector;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
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
    let projector = Arc::new(SkillProjector::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let config = Arc::new(AgentConfig::default());
    let meta_pool = pool.with_database("evotai_meta").expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm.clone()));
    Arc::new(Session::new(
        id.into(),
        "a1".into(),
        "u1".into(),
        SessionResources {
            workspace,
            tool_registry: Arc::new(ToolRegistry::new()),
            org,
            tools: Arc::new(vec![]),
            storage: Arc::new(AgentStore::new(pool, llm.clone())),
            llm: Arc::new(RwLock::new(llm)),
            config,
            prompt_variables: vec![],
            cluster_client: None,
            directive: None,
            trace_writer: bendclaw::kernel::trace::TraceWriter::noop(),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            prompt_config: None,
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
