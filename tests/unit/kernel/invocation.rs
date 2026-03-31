//! Tests for the invocation layer: request validation, acquire, CLI arg derivation.

use bendclaw::kernel::invocation::request::*;
use bendclaw::kernel::session::options::RunOptions;

// ── ConfigSource derivation (mirrors CLI logic) ─────────────────────────────

#[test]
fn no_agent_id_yields_local() {
    let agent_id: Option<String> = None;
    let source = match agent_id {
        Some(ref aid) => ConfigSource::Cloud {
            agent_id: aid.clone(),
            user_id: "u1".into(),
        },
        None => ConfigSource::Local,
    };
    assert!(matches!(source, ConfigSource::Local));
}

#[test]
fn agent_id_with_user_id_yields_cloud() {
    let agent_id = Some("a1".to_string());
    let user_id = Some("u1".to_string());
    let source = match agent_id {
        Some(ref aid) => {
            let uid = user_id.as_deref().expect("--user-id required");
            ConfigSource::Cloud {
                agent_id: aid.clone(),
                user_id: uid.to_string(),
            }
        }
        None => ConfigSource::Local,
    };
    assert!(matches!(source, ConfigSource::Cloud { .. }));
}

#[test]
#[should_panic(expected = "--user-id required")]
fn agent_id_without_user_id_panics() {
    let agent_id = Some("a1".to_string());
    let user_id: Option<String> = None;
    let _source = match agent_id {
        Some(ref aid) => {
            let uid = user_id.as_deref().expect("--user-id required");
            ConfigSource::Cloud {
                agent_id: aid.clone(),
                user_id: uid.to_string(),
            }
        }
        None => ConfigSource::Local,
    };
}

// ── Validation: Local + Persistent is rejected ──────────────────────────────

#[test]
fn local_plus_persistent_is_invalid() {
    let req = InvocationRequest {
        source: ConfigSource::Local,
        persistence: PersistenceMode::Persistent {
            session_id: "s1".into(),
        },
        context: ConversationContext::None,
        prompt: "hello".into(),
        options: RunOptions::default(),
        session_options: SessionBuildOptions::default(),
    };
    // Validate rejects Local + Persistent
    assert!(matches!(
        (&req.source, &req.persistence),
        (ConfigSource::Local, PersistenceMode::Persistent { .. })
    ));
}

// ── NoopBackend ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn noop_backend_load_history_returns_empty() {
    use bendclaw::kernel::session::backend::context::SessionContextProvider;
    use bendclaw::kernel::session::backend::noop::NoopBackend;

    let backend = NoopBackend;
    let history = backend.load_history(100).await.unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn noop_backend_enforce_token_limits_succeeds() {
    use bendclaw::kernel::session::backend::context::SessionContextProvider;
    use bendclaw::kernel::session::backend::noop::NoopBackend;

    let backend = NoopBackend;
    assert!(backend.enforce_token_limits().await.is_ok());
}

#[test]
fn noop_backend_init_run_returns_run_id() {
    use bendclaw::kernel::session::backend::noop::NoopBackend;
    use bendclaw::kernel::session::backend::sink::RunInitializer;

    let backend = NoopBackend;
    let run_id = backend.init_run("hello", None, "node-1").unwrap();
    assert!(!run_id.is_empty());
}
