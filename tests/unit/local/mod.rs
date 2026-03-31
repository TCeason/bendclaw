//! Tests for local assembly — verifies directory layout and assembly boundaries.

use std::sync::Arc;

use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::assembly::local::build_local_assembly;
use bendclaw::kernel::session::assembly::local::LocalBuildOptions;
use bendclaw::kernel::session::assembly::local::LocalRuntimeDeps;

fn test_deps(root_dir: &std::path::Path) -> LocalRuntimeDeps {
    let llm = Arc::new(bendclaw_test_harness::mocks::llm::MockLLMProvider::with_text("ok"))
        as Arc<dyn bendclaw::llm::provider::LLMProvider>;
    let mut deps = LocalRuntimeDeps::new(AgentConfig::default(), llm);
    deps.config.workspace.root_dir = root_dir.to_string_lossy().to_string();
    deps
}

#[tokio::test]
async fn local_directory_layout_is_correct() {
    let dir = tempfile::tempdir().unwrap();
    let deps = test_deps(dir.path());

    let assembly = build_local_assembly(&deps, "test-session", LocalBuildOptions {
        cwd: Some(dir.path().to_path_buf()),
        tool_filter: None,
        llm_override: None,
    })
    .expect("assembly should succeed");

    // session_root = {root_dir}/local/sessions/{session_id}
    let session_root = dir
        .path()
        .join("local")
        .join("sessions")
        .join("test-session");
    assert!(
        session_root.exists(),
        "session_root should exist: {session_root:?}"
    );

    // workspace.dir = session_root/workspace
    let ws_dir = assembly.core.workspace.dir().to_path_buf();
    assert_eq!(ws_dir, session_root.join("workspace"));
    assert!(ws_dir.exists(), "workspace dir should exist");

    // No user_id/agent_id nesting
    assert!(
        !dir.path().join("cli").exists(),
        "should not have user_id dir"
    );
    assert!(
        !dir.path().join("local").join("local").exists(),
        "should not have agent_id dir"
    );

    // No double session_id nesting
    assert!(
        !session_root.join("test-session").exists(),
        "should not have double session_id nesting"
    );

    // Store writes land at session_root level (not inside workspace/)
    let record = bendclaw::storage::dal::run::record::RunRecord {
        id: "r1".to_string(),
        session_id: "test-session".to_string(),
        agent_id: "local".to_string(),
        user_id: "cli".to_string(),
        kind: "user_turn".to_string(),
        parent_run_id: String::new(),
        node_id: String::new(),
        status: "RUNNING".to_string(),
        input: "hello".to_string(),
        output: String::new(),
        error: String::new(),
        metrics: String::new(),
        stop_reason: String::new(),
        checkpoint_through_run_id: String::new(),
        iterations: 0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    assembly.infra.store.run_insert(&record).await.unwrap();

    // Run file at session_root/runs/r1.json
    assert!(
        session_root.join("runs").join("r1.json").exists(),
        "run file should be at session_root/runs/r1.json"
    );
    // NOT inside workspace/
    assert!(
        !ws_dir.join("runs").exists(),
        "runs should not be inside workspace/"
    );
}

#[tokio::test]
async fn local_assembly_persistent_backend_creates_run() {
    let dir = tempfile::tempdir().unwrap();
    let deps = test_deps(dir.path());

    let assembly = build_local_assembly(&deps, "s1", LocalBuildOptions {
        cwd: Some(dir.path().to_path_buf()),
        tool_filter: None,
        llm_override: None,
    })
    .expect("assembly should succeed");

    let run_id = assembly
        .core
        .run_initializer
        .init_run("hello", None, "node-1")
        .expect("init_run should succeed");
    assert!(!run_id.is_empty(), "run_id should not be empty");
}

#[tokio::test]
async fn sandbox_mode_cwd_defaults_to_workspace_dir() {
    let dir = tempfile::tempdir().unwrap();
    let mut deps = test_deps(dir.path());
    deps.config.workspace.sandbox = true;

    // No explicit cwd — sandbox should default to workspace_dir
    let assembly = build_local_assembly(&deps, "sandbox-session", LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: None,
    })
    .expect("assembly should succeed");

    let ws_dir = assembly.core.workspace.dir().to_path_buf();
    let cwd = assembly.core.workspace.cwd().to_path_buf();

    // cwd must equal workspace_dir when sandbox=true and no explicit cwd
    assert_eq!(
        cwd, ws_dir,
        "sandbox cwd should be workspace_dir, not an external path"
    );
}

#[tokio::test]
async fn sandbox_mode_ignores_explicit_external_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let mut deps = test_deps(dir.path());
    deps.config.workspace.sandbox = true;

    let external_dir = tempfile::tempdir().unwrap();

    let assembly = build_local_assembly(&deps, "sandbox-ext", LocalBuildOptions {
        cwd: Some(external_dir.path().to_path_buf()),
        tool_filter: None,
        llm_override: None,
    })
    .expect("assembly should succeed");

    let ws_dir = assembly.core.workspace.dir().to_path_buf();
    let cwd = assembly.core.workspace.cwd().to_path_buf();

    // Even with explicit external cwd, sandbox must clamp to workspace_dir
    assert_eq!(cwd, ws_dir, "sandbox must ignore external cwd override");
}

#[tokio::test]
async fn no_cwd_override_open_mode_uses_home() {
    let dir = tempfile::tempdir().unwrap();
    let mut deps = test_deps(dir.path());
    deps.config.workspace.sandbox = false;

    let assembly = build_local_assembly(&deps, "open-session", LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: None,
    })
    .expect("assembly should succeed");

    let ws_dir = assembly.core.workspace.dir().to_path_buf();
    let cwd = assembly.core.workspace.cwd().to_path_buf();

    // In open mode without explicit cwd, cwd should be HOME (not workspace_dir)
    assert_ne!(cwd, ws_dir, "open-mode cwd should not be workspace_dir");
}
