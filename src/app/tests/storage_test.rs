use evot::agent::*;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
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
    storage.save_session(session_meta).await?;
    assert!(storage.get_session("sess-backend").await?.is_some());

    storage
        .append_entry(TranscriptEntry::new(
            "sess-backend".into(),
            None,
            1,
            0,
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
        ))
        .await?;
    storage
        .append_entry(TranscriptEntry::new(
            "sess-backend".into(),
            None,
            2,
            0,
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text { text: "hi".into() }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ))
        .await?;

    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-backend".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 2);
    Ok(())
}

#[tokio::test]
async fn list_entries_skips_corrupt_line() -> TestResult {
    // A single unparseable transcript line must not make the whole session
    // unloadable. Canonical assistant lines around it still load normally.
    let root = TempDir::new()?;
    let session_dir = root.path().join("sessions").join("sess-legacy");
    std::fs::create_dir_all(&session_dir)?;

    let assistant = r#"{"session_id":"sess-legacy","run_id":null,"seq":1,"turn":0,"item":{"type":"assistant","content":[{"type":"text","text":"hi"}],"stop_reason":"stop","usage":{"input":0,"output":0,"cache_read":0,"cache_write":0},"model":"model","provider":"provider","timestamp":1},"created_at":"2026-04-23T07:10:17Z"}"#;
    let corrupt = r#"{ this is not valid json "#;
    let user_line = r#"{"session_id":"sess-legacy","run_id":null,"seq":2,"turn":0,"item":{"type":"user","text":"hello"},"created_at":"2026-04-23T07:10:18Z"}"#;
    std::fs::write(
        session_dir.join("transcript.jsonl"),
        format!("{assistant}\n{corrupt}\n{user_line}\n"),
    )?;

    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;
    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-legacy".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;

    // Corrupt line skipped; both canonical lines loaded.
    assert_eq!(loaded.len(), 2);
    assert!(matches!(loaded[0].item, TranscriptItem::Assistant { .. }));
    assert!(matches!(loaded[1].item, TranscriptItem::User { .. }));
    Ok(())
}
