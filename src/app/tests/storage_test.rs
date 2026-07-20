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
async fn list_entries_reads_legacy_object_lines_and_skips_corruption() -> TestResult {
    // Historical transcripts stored one entry per line. Current transcripts
    // store an atomic array batch per line; both shapes must remain readable.
    let root = TempDir::new()?;
    let session_dir = root.path().join("sessions").join("sess-legacy");
    std::fs::create_dir_all(&session_dir)?;

    let assistant = r#"{"session_id":"sess-legacy","run_id":null,"seq":1,"turn":0,"item":{"type":"assistant","text":"hi","thinking":"plan","tool_calls":[],"stop_reason":"stop","usage":{"input":0,"output":0,"cache_read":0,"cache_write":0},"model":"model","provider":"provider","timestamp":1},"created_at":"2026-04-23T07:10:17Z"}"#;
    let corrupt = r#"[{ this batch was interrupted "#;
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

    assert_eq!(loaded.len(), 2);
    assert!(matches!(
        &loaded[0].item,
        TranscriptItem::Assistant { content, .. }
            if matches!(
                &content[..],
                [AssistantBlock::Thinking { text: thinking, .. }, AssistantBlock::Text { text }]
                    if thinking == "plan" && text == "hi"
            )
    ));
    assert!(matches!(&loaded[1].item, TranscriptItem::User { text, .. } if text == "hello"));
    Ok(())
}

#[tokio::test]
async fn append_continues_after_legacy_object_entries() -> TestResult {
    let root = TempDir::new()?;
    let session_id = "sess-legacy-append";
    let session_dir = root.path().join("sessions").join(session_id);
    std::fs::create_dir_all(&session_dir)?;
    let legacy = format!(
        "{{\"session_id\":\"{session_id}\",\"run_id\":null,\"seq\":41,\"turn\":0,\"item\":{{\"type\":\"user\",\"text\":\"old\"}},\"created_at\":\"2026-04-23T07:10:18Z\"}}\n"
    );
    std::fs::write(session_dir.join("transcript.jsonl"), legacy)?;

    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;
    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: session_id.into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].seq, 1);
    assert!(session_dir.join("transcript.jsonl.v1.bak").exists());

    let accepted = storage
        .compare_and_append_entries(1, vec![TranscriptEntry::new(
            session_id.into(),
            None,
            2,
            0,
            TranscriptItem::User {
                text: "new".into(),
                content: vec![],
            },
        )])
        .await?;
    assert!(accepted);

    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: session_id.into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(
        loaded.iter().map(|entry| entry.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
    Ok(())
}

#[tokio::test]
async fn mixed_transcript_is_migrated_and_reset_sequences_are_repaired() -> TestResult {
    let root = TempDir::new()?;
    let session_id = "sess-mixed-reset";
    let session_dir = root.path().join("sessions").join(session_id);
    std::fs::create_dir_all(&session_dir)?;
    let legacy = format!(
        "{{\"session_id\":\"{session_id}\",\"run_id\":null,\"seq\":100,\"turn\":0,\"item\":{{\"type\":\"user\",\"text\":\"old\"}},\"created_at\":\"2026-04-23T07:10:18Z\"}}\n"
    );
    let reset_batch = format!(
        "[{{\"session_id\":\"{session_id}\",\"run_id\":null,\"seq\":1,\"turn\":0,\"item\":{{\"type\":\"user\",\"text\":\"new one\"}},\"created_at\":\"2026-04-23T07:10:19Z\"}},{{\"session_id\":\"{session_id}\",\"run_id\":null,\"seq\":2,\"turn\":0,\"item\":{{\"type\":\"user\",\"text\":\"new two\"}},\"created_at\":\"2026-04-23T07:10:20Z\"}}]\n"
    );
    std::fs::write(
        session_dir.join("transcript.jsonl"),
        format!("{legacy}{reset_batch}"),
    )?;

    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;
    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: session_id.into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(
        loaded.iter().map(|entry| entry.seq).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(session_dir.join("transcript.jsonl.v1.bak").exists());
    let first_line = std::fs::read_to_string(session_dir.join("transcript.jsonl"))?
        .lines()
        .next()
        .ok_or_else(|| std::io::Error::other("missing migrated batch"))?
        .to_string();
    assert!(serde_json::from_str::<serde_json::Value>(&first_line)?.is_array());
    assert!(
        storage
            .compare_and_append_entries(3, vec![TranscriptEntry::new(
                session_id.into(),
                None,
                4,
                0,
                TranscriptItem::User {
                    text: "continued".into(),
                    content: vec![],
                },
            )],)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn compare_and_append_rejects_non_contiguous_first_sequence() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;
    let accepted = storage
        .compare_and_append_entries(0, vec![TranscriptEntry::new(
            "sess-bad-first-seq".into(),
            None,
            2,
            0,
            TranscriptItem::User {
                text: "must not persist".into(),
                content: vec![],
            },
        )])
        .await?;
    assert!(!accepted);
    let entries = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-bad-first-seq".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert!(entries.is_empty());
    Ok(())
}

#[tokio::test]
async fn append_repairs_interrupted_tail_before_writing_next_batch() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;
    let session_id = "sess-tail-repair";
    storage
        .save_session(SessionMeta::new(
            session_id.into(),
            "/tmp".into(),
            "model".into(),
        ))
        .await?;
    storage
        .append_entry(TranscriptEntry::new(
            session_id.into(),
            None,
            1,
            0,
            TranscriptItem::User {
                text: "before crash".into(),
                content: vec![],
            },
        ))
        .await?;

    let transcript_path = root
        .path()
        .join("sessions")
        .join(session_id)
        .join("transcript.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript_path)?;
    file.write_all(b"[{\"interrupted\":")?;
    drop(file);

    storage
        .append_entry(TranscriptEntry::new(
            session_id.into(),
            None,
            2,
            0,
            TranscriptItem::User {
                text: "after recovery".into(),
                content: vec![],
            },
        ))
        .await?;

    let entries = storage
        .list_entries(ListTranscriptEntries {
            session_id: session_id.into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].seq, 1);
    assert_eq!(entries[1].seq, 2);
    assert!(matches!(
        &entries[1].item,
        TranscriptItem::User { text, .. } if text == "after recovery"
    ));
    Ok(())
}
