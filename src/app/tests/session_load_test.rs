use bendclaw::conf::StorageConfig;
use bendclaw::session::load_session;
use bendclaw::session::new_session;
use bendclaw::session::save_transcript;
use bendclaw::session::update_transcript;
use bendclaw::storage::open_storage;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

#[tokio::test]
async fn new_session_creates_meta_and_empty_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let state = new_session(
        "sess-100".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.as_ref(),
    )
    .await?;

    assert_eq!(state.meta.session_id, "sess-100");
    assert_eq!(state.meta.turns, 0);
    assert!(state.messages.is_empty());
    assert!(dir
        .path()
        .join("sessions")
        .join("sess-100")
        .join("session.json")
        .exists());
    Ok(())
}

#[tokio::test]
async fn load_session_returns_none_for_missing() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let state = load_session("nonexistent", storage.as_ref()).await?;
    assert!(state.is_none());
    Ok(())
}

#[tokio::test]
async fn round_trip_session_with_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let mut state = new_session(
        "sess-200".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.as_ref(),
    )
    .await?;

    update_transcript(&mut state, vec![
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
        },
    ]);

    save_transcript(&state, storage.as_ref()).await?;

    let loaded = load_session("sess-200", storage.as_ref())
        .await?
        .ok_or_else(|| missing_error("missing loaded session"))?;
    assert_eq!(loaded.meta.turns, 1);
    assert_eq!(loaded.messages.len(), 2);
    Ok(())
}

#[tokio::test]
async fn resume_session_appends_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let mut state = new_session(
        "sess-300".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.as_ref(),
    )
    .await?;

    update_transcript(&mut state, vec![bend_agent::Message {
        role: bend_agent::MessageRole::User,
        content: vec![bend_agent::ContentBlock::Text {
            text: "first".into(),
        }],
    }]);
    save_transcript(&state, storage.as_ref()).await?;

    let mut resumed = load_session("sess-300", storage.as_ref())
        .await?
        .ok_or_else(|| missing_error("missing resumed session"))?;

    let mut extended = resumed.messages.clone();
    extended.push(bend_agent::Message {
        role: bend_agent::MessageRole::User,
        content: vec![bend_agent::ContentBlock::Text {
            text: "second".into(),
        }],
    });
    extended.push(bend_agent::Message {
        role: bend_agent::MessageRole::Assistant,
        content: vec![bend_agent::ContentBlock::Text {
            text: "reply".into(),
        }],
    });

    update_transcript(&mut resumed, extended);
    save_transcript(&resumed, storage.as_ref()).await?;

    let final_state = load_session("sess-300", storage.as_ref())
        .await?
        .ok_or_else(|| missing_error("missing final state"))?;
    assert_eq!(final_state.messages.len(), 3);
    assert_eq!(final_state.meta.turns, 2);
    Ok(())
}
