use bendclaw::conf::StorageConfig;
use bendclaw::run::RunEvent;
use bendclaw::run::RunEventKind;
use bendclaw::run::RunMeta;
use bendclaw::session::SessionMeta;
use bendclaw::storage::model::ListRunEvents;
use bendclaw::storage::open_storage;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn open_storage_returns_working_backend() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;

    let session_meta = SessionMeta::new(
        "sess-backend".into(),
        "/tmp".into(),
        "claude-sonnet-4-20250514".into(),
    );
    storage.put_session(session_meta).await?;
    assert!(storage.get_session("sess-backend").await?.is_some());

    let run_meta = RunMeta::new(
        "run-backend".into(),
        "sess-backend".into(),
        "claude-sonnet-4-20250514".into(),
    );
    storage.put_run(run_meta).await?;

    let event = RunEvent::new(
        "run-backend".into(),
        "sess-backend".into(),
        0,
        RunEventKind::RunStarted,
        serde_json::json!({}),
    );
    storage.put_run_events(vec![event]).await?;

    let loaded_events = storage
        .list_run_events(ListRunEvents {
            run_id: "run-backend".into(),
        })
        .await?;
    assert_eq!(loaded_events.len(), 1);
    Ok(())
}
