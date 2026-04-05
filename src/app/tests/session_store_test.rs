use bendclaw::conf::StoreConfig;
use bendclaw::session::SessionMeta;
use bendclaw::store::create_stores;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

#[tokio::test]
async fn save_and_load_meta() -> TestResult {
    let dir = TempDir::new()?;
    let stores = create_stores(&StoreConfig::fs(dir.path().to_path_buf()))?;

    let meta = SessionMeta::new("sess-001".into(), "/tmp".into(), "claude-sonnet".into());
    stores.session.save_meta(&meta).await?;

    let loaded = stores
        .session
        .load_meta("sess-001")
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
    let stores = create_stores(&StoreConfig::fs(dir.path().to_path_buf()))?;

    let loaded = stores.session.load_meta("nonexistent").await?;
    assert!(loaded.is_none());
    Ok(())
}

#[tokio::test]
async fn save_and_load_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let stores = create_stores(&StoreConfig::fs(dir.path().to_path_buf()))?;

    let messages = vec![
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hi there".into(),
            }],
        },
    ];

    stores
        .session
        .save_transcript("sess-002", &messages)
        .await?;

    let loaded = stores
        .session
        .load_transcript("sess-002")
        .await?
        .ok_or_else(|| missing_error("missing transcript"))?;
    assert_eq!(loaded.len(), 2);
    Ok(())
}

#[tokio::test]
async fn load_transcript_not_found() -> TestResult {
    let dir = TempDir::new()?;
    let stores = create_stores(&StoreConfig::fs(dir.path().to_path_buf()))?;

    let loaded = stores.session.load_transcript("nonexistent").await?;
    assert!(loaded.is_none());
    Ok(())
}

#[tokio::test]
async fn list_recent_sessions() -> TestResult {
    let dir = TempDir::new()?;
    let stores = create_stores(&StoreConfig::fs(dir.path().to_path_buf()))?;

    for index in 0..5 {
        let meta = SessionMeta::new(
            format!("sess-{index:03}"),
            "/tmp".into(),
            "claude-sonnet".into(),
        );
        stores.session.save_meta(&meta).await?;
    }

    let recent = stores.session.list_recent(3).await?;
    assert_eq!(recent.len(), 3);
    Ok(())
}
