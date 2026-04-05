use bendclaw::conf::StorageConfig;
use bendclaw::session::SessionMeta;
use bendclaw::storage::model::ListSessions;
use bendclaw::storage::model::ListTranscriptEntries;
use bendclaw::storage::model::TranscriptEntry;
use bendclaw::storage::open_storage;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

#[tokio::test]
async fn save_and_load_meta() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let meta = SessionMeta::new("sess-001".into(), "/tmp".into(), "claude-sonnet".into());
    storage.put_session(meta).await?;

    let loaded = storage
        .get_session("sess-001")
        .await?
        .ok_or_else(|| missing_error("missing session meta"))?;
    assert_eq!(loaded.session_id, "sess-001");
    assert_eq!(loaded.cwd, "/tmp");
    assert_eq!(loaded.model, "claude-sonnet");
    assert_eq!(loaded.turns, 0);
    Ok(())
}

#[tokio::test]
async fn load_meta_not_found() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let loaded = storage.get_session("nonexistent").await?;
    assert!(loaded.is_none());
    Ok(())
}

#[tokio::test]
async fn save_and_load_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let entries = vec![
        TranscriptEntry::new("sess-002".into(), None, 1, 0, bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        }),
        TranscriptEntry::new("sess-002".into(), None, 2, 0, bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hi there".into(),
            }],
        }),
    ];

    storage.put_transcript_entries(entries).await?;

    let loaded = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: "sess-002".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 2);
    Ok(())
}

#[tokio::test]
async fn load_transcript_not_found() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let loaded = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: "nonexistent".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert!(loaded.is_empty());
    Ok(())
}

#[tokio::test]
async fn list_recent_sessions() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    for index in 0..5 {
        let meta = SessionMeta::new(
            format!("sess-{index:03}"),
            "/tmp".into(),
            "claude-sonnet".into(),
        );
        storage.put_session(meta).await?;
    }

    let recent = storage.list_sessions(ListSessions { limit: 3 }).await?;
    assert_eq!(recent.len(), 3);
    Ok(())
}
