use evot::agent::session::Session;
use evot::agent::*;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

fn compact_item(summary: &str, generation: u32) -> TranscriptItem {
    TranscriptItem::Compact {
        id: format!("compact-{generation}"),
        created_at: 0,
        reason: evot::types::CompactReason::Manual,
        summary: summary.into(),
        tokens_before: 100,
        tokens_after: 10,
        messages_before: 4,
        messages_after: 2,
        messages: vec![],
        engine_messages: vec![],
        state: Box::new(evot_engine::CompactionState {
            generation,
            last_summary: Some(summary.into()),
            context_summary_message: Some(evot::compact::context_view::compact_summary_text(
                summary,
            )),
            ..Default::default()
        }),
        details: evot::types::CompactDetails::default(),
    }
}

async fn write_test_compact(
    session: &Session,
    summary: &str,
    new_context: Vec<TranscriptItem>,
) -> TestResult {
    let (_, previous_state, expected_seq) = session.context_snapshot().await;
    let generation = previous_state
        .as_ref()
        .map(|state| state.generation.saturating_add(1))
        .unwrap_or(1);
    let mut item = compact_item(summary, generation);
    if let TranscriptItem::Compact { state, .. } = &mut item {
        if let Some(previous) = previous_state {
            state.file_ops = previous.file_ops;
        }
    }
    if let TranscriptItem::Compact {
        messages,
        engine_messages,
        ..
    } = &mut item
    {
        *messages = new_context.clone();
        *engine_messages = evot::agent::run::convert::into_agent_messages(&new_context);
    }
    session
        .write_compact(item, new_context, expected_seq)
        .await?;
    Ok(())
}

#[tokio::test]
async fn new_session_creates_meta_and_empty_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-100".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    let meta = session.meta().await;
    let transcript = session.transcript().await;
    assert_eq!(meta.session_id, "sess-100");
    assert_eq!(meta.turns, 0);
    assert!(transcript.is_empty());
    assert!(dir
        .path()
        .join("sessions")
        .join("sess-100")
        .join("session.json")
        .exists());
    Ok(())
}

#[tokio::test]
async fn model_selection_update_is_persisted_immediately() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new_with_provider_source(
        "selection-persist".into(),
        "/tmp".into(),
        "provider-a".into(),
        "shared-model".into(),
        "repl",
        storage.clone(),
    )
    .await?;

    session
        .set_model_selection("provider-b".into(), "shared-model".into())
        .await?;
    // Reapplying the active selection (the normal per-submit path) must not
    // create duplicate audit entries.
    session
        .set_model_selection("provider-b".into(), "shared-model".into())
        .await?;

    let raw = session.load_all_entries().await?;
    let changes: Vec<_> = raw
        .iter()
        .filter_map(|entry| match &entry.item {
            TranscriptItem::Stats { kind, data } if kind == "model_change" => Some(data),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["from_provider"], "provider-a");
    assert_eq!(changes[0]["to_provider"], "provider-b");

    let reopened = Session::open("selection-persist", storage)
        .await?
        .ok_or_else(|| missing_error("missing reopened session"))?;
    let meta = reopened.meta().await;
    assert_eq!(meta.provider, "provider-b");
    assert_eq!(meta.model, "shared-model");
    // Audit facts remain append-only but never enter LLM context.
    assert!(reopened.transcript().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn agent_create_session_persists_empty_repl_session() -> TestResult {
    let dir = TempDir::new()?;
    let mut config = evot::conf::Config::new(dir.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "http://localhost".into(),
        models: vec!["test-model".into()],
        compat_caps: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();

    let agent = Agent::new(&config, "/work")?;
    let meta = agent.create_session("repl").await?;

    assert_eq!(meta.cwd, "/work");
    assert_eq!(meta.model, "test-model");
    assert_eq!(meta.provider, "test");
    assert_eq!(meta.source, "repl");
    assert_eq!(meta.turns, 0);

    let loaded = agent
        .find_session(&meta.session_id)
        .await?
        .ok_or_else(|| missing_error("missing created session"))?;
    assert_eq!(loaded.session_id, meta.session_id);

    let transcript = agent.load_transcript(&meta.session_id).await?;
    assert!(transcript.is_empty());

    let sessions = agent.list_sessions(0).await?;
    assert!(sessions.iter().any(|s| s.session_id == meta.session_id));
    Ok(())
}

#[tokio::test]
async fn open_session_returns_none_for_missing() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::open("nonexistent", storage.clone()).await?;
    assert!(session.is_none());
    Ok(())
}

/// A missing API key must NOT block startup: the agent constructs fine and a
/// session can be created. The failure is deferred to query time, where it is
/// surfaced as a visible error event pointing the user at settings.
#[tokio::test]
async fn missing_api_key_defers_to_query_time() -> TestResult {
    let dir = TempDir::new()?;
    let mut config = evot::conf::Config::new(dir.path().to_path_buf());
    config
        .providers
        .insert("anthropic".into(), ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "".into(), // <- the key the dashboard has not set yet
            base_url: "http://localhost".into(),
            models: vec!["claude-sonnet".into()],
            compat_caps: Default::default(),
            thinking_level: None,
            context_window: None,
            max_tokens: None,
            supports_image: None,
        });
    config.llm.provider = "anthropic".into();

    // Construction must succeed despite the empty key (no startup gate).
    let agent = Agent::new(&config, "/work")?;
    let session = agent.create_session("repl").await?;
    let loaded = agent
        .load_session(&session.session_id)
        .await?
        .ok_or_else(|| missing_error("missing session"))?;

    // The error appears at query time as a visible Error event, not a panic
    // or a silent finish.
    let outcome = agent
        .submit_to_session(QueryRequest::text("hello"), loaded)
        .await?;
    let mut run = match outcome {
        SubmitOutcome::Run(run) => run,
        SubmitOutcome::Command(message) => {
            return Err(missing_error(&format!("unexpected command: {message}")).into())
        }
    };

    let mut error_message = None;
    while let Some(event) = run.next().await {
        if let RunEventPayload::Error { message } = event.payload {
            error_message = Some(message);
        }
    }

    let message = error_message.ok_or_else(|| missing_error("expected an error event"))?;
    assert!(
        message.contains("API key") && message.contains("anthropic"),
        "error should name the missing key and provider: {message}"
    );
    Ok(())
}

/// Fresh-install path: no providers configured at all (the default env file is
/// fully commented out). The agent must still construct and the failure must
/// surface at query time as a visible error pointing at configuration — not a
/// startup crash or a `provider '' not found` panic.
#[tokio::test]
async fn no_provider_configured_defers_to_query_time() -> TestResult {
    let dir = TempDir::new()?;
    // Config::new leaves `providers` empty and `llm.provider` blank, exactly
    // like a brand-new install before any key is entered.
    let config = evot::conf::Config::new(dir.path().to_path_buf());

    // Construction must succeed despite zero providers (no startup gate).
    let agent = Agent::new(&config, "/work")?;
    let session = agent.create_session("repl").await?;
    let loaded = agent
        .load_session(&session.session_id)
        .await?
        .ok_or_else(|| missing_error("missing session"))?;

    let outcome = agent
        .submit_to_session(QueryRequest::text("hello"), loaded)
        .await?;
    let mut run = match outcome {
        SubmitOutcome::Run(run) => run,
        SubmitOutcome::Command(message) => {
            return Err(missing_error(&format!("unexpected command: {message}")).into())
        }
    };

    let mut error_message = None;
    while let Some(event) = run.next().await {
        if let RunEventPayload::Error { message } = event.payload {
            error_message = Some(message);
        }
    }

    let message = error_message.ok_or_else(|| missing_error("expected an error event"))?;
    assert!(
        message.contains("provider"),
        "error should point at provider configuration: {message}"
    );
    Ok(())
}

#[tokio::test]
async fn round_trip_session_with_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-200".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text { text: "hi".into() }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    let loaded = Session::open("sess-200", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing loaded session"))?;
    assert_eq!(loaded.meta().await.turns, 0);
    assert_eq!(loaded.transcript().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn resume_session_appends_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-300".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "first".into(),
            content: vec![],
        }])
        .await?;

    let resumed = Session::open("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing resumed session"))?;

    resumed
        .write_items(vec![
            TranscriptItem::User {
                text: "second".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "reply".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    let final_state = Session::open("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing final state"))?;
    assert_eq!(final_state.transcript().await.len(), 3);
    assert_eq!(final_state.meta().await.turns, 0);
    Ok(())
}

#[tokio::test]
async fn session_title_comes_from_first_user_message() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-title".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "summarize the quarterly numbers for the infra team".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "working".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;
    session.save().await?;

    let loaded = Session::open("sess-title", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing titled session"))?;
    let title = loaded
        .meta()
        .await
        .title
        .ok_or_else(|| missing_error("missing session title"))?;

    assert_eq!(title, "summarize the quarterly numbers for the infra team");
    Ok(())
}

#[tokio::test]
async fn save_and_load_meta() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let meta = SessionMeta::new("sess-001".into(), "/tmp".into(), "claude-sonnet".into());
    storage.save_session(meta).await?;

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

// --- PLACEHOLDER_REST ---

#[tokio::test]
async fn save_and_load_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    storage
        .append_entry(TranscriptEntry::new(
            "sess-002".into(),
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
            "sess-002".into(),
            None,
            2,
            0,
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "hi there".into(),
                }],
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
            session_id: "sess-002".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 2);
    assert!(matches!(&loaded[0].item, TranscriptItem::User { text, .. } if text == "hello"));
    assert!(
        matches!(&loaded[1].item, TranscriptItem::Assistant { content, ..} if matches!(&content[..], [AssistantBlock::Text { text }] if text == "hi there"))
    );
    Ok(())
}

#[tokio::test]
async fn open_resumes_from_last_compact_entry() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-compact".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old message 1".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "old reply 1".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
            TranscriptItem::User {
                text: "old message 2".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "old reply 2".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    // Commit a Compact control point atomically.
    write_test_compact(&session, "summary of prior context", vec![
        evot::compact::context_view::compact_summary_item("summary of prior context"),
        TranscriptItem::User {
            text: "old message 2".into(),
            content: vec![],
        },
        TranscriptItem::Assistant {
            content: vec![AssistantBlock::Text {
                text: "old reply 2".into(),
            }],
            stop_reason: "stop".into(),
            usage: UsageSummary::default(),
            model: String::new(),
            provider: String::new(),
            timestamp: 0,
            error_message: None,
        },
    ])
    .await?;

    // Append more messages after compaction
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "new message after compact".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "new reply".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    // Load — should resume from the structured compact boundary
    let loaded = Session::open("sess-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing compacted session"))?;
    let transcript = loaded.transcript().await;

    // Should have: compact summary + retained snapshot + new messages.
    assert_eq!(transcript.len(), 5);
    assert!(
        matches!(&transcript[0], TranscriptItem::User { text, .. } if text.contains("summary of prior context"))
    );
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "old message 2"));
    assert!(
        matches!(&transcript[2], TranscriptItem::Assistant { content, ..} if matches!(&content[..], [AssistantBlock::Text { text }] if text == "old reply 2"))
    );
    assert!(
        matches!(&transcript[3], TranscriptItem::User { text, .. } if text == "new message after compact")
    );
    Ok(())
}

#[tokio::test]
async fn compaction_seed_updates_restores_and_clear_breaks_chain() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-compact-seed".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    let mut first = compact_item("summary-v1", 1);
    if let TranscriptItem::Compact {
        details,
        messages,
        engine_messages,
        state,
        ..
    } = &mut first
    {
        details.read_files.push("src/read.rs".into());
        details.modified_files.push("src/edited.rs".into());
        state.file_ops.read.insert("src/read.rs".into());
        state.file_ops.edited.insert("src/edited.rs".into());
        *messages = vec![evot::compact::context_view::compact_summary_item(
            "summary-v1",
        )];
        *engine_messages = evot::agent::run::convert::into_agent_messages(messages);
    }
    session
        .write_compact(
            first,
            vec![evot::compact::context_view::compact_summary_item(
                "summary-v1",
            )],
            0,
        )
        .await?;

    let seed = session
        .compaction_seed()
        .await
        .ok_or_else(|| missing_error("missing in-process compaction seed"))?;
    assert_eq!(seed.generation, 1);
    assert_eq!(seed.last_summary.as_deref(), Some("summary-v1"));
    assert!(seed.file_ops.read.contains("src/read.rs"));
    assert!(seed.file_ops.edited.contains("src/edited.rs"));

    write_test_compact(&session, "summary-v2", vec![
        evot::compact::context_view::compact_summary_item("summary-v2"),
    ])
    .await?;
    assert_eq!(
        session
            .compaction_seed()
            .await
            .map(|state| state.generation),
        Some(2)
    );

    drop(session);
    let reopened = Session::open("sess-compact-seed", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing compacted session"))?;
    let restored = reopened
        .compaction_seed()
        .await
        .ok_or_else(|| missing_error("missing restored compaction seed"))?;
    assert_eq!(restored.generation, 2);
    assert_eq!(restored.last_summary.as_deref(), Some("summary-v2"));

    reopened.write_clear_marker().await?;
    assert!(reopened.compaction_seed().await.is_none());
    drop(reopened);

    let after_clear = Session::open("sess-compact-seed", storage)
        .await?
        .ok_or_else(|| missing_error("missing cleared session"))?;
    assert!(after_clear.compaction_seed().await.is_none());
    Ok(())
}

#[tokio::test]
async fn open_without_compact_returns_all_entries() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-no-compact".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text { text: "hi".into() }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    let loaded = Session::open("sess-no-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "hello"));
    assert!(
        matches!(&transcript[1], TranscriptItem::Assistant { content, ..} if matches!(&content[..], [AssistantBlock::Text { text }] if text == "hi"))
    );
    Ok(())
}

#[tokio::test]
async fn write_items_is_append_only() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-append".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "first".into(),
            content: vec![],
        }])
        .await?;

    write_test_compact(&session, "compacted", vec![
        evot::compact::context_view::compact_summary_item("compacted"),
    ])
    .await?;

    // Raw storage should have 2 entries (User + Compact), not a rewrite
    let raw = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-append".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(raw.len(), 2);
    assert!(matches!(&raw[0].item, TranscriptItem::User { .. }));
    assert!(matches!(&raw[1].item, TranscriptItem::Compact { .. }));
    Ok(())
}

#[tokio::test]
async fn failed_batch_does_not_publish_session_state_or_advance_sequence() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-failed-batch".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    let transcript_path = dir
        .path()
        .join("sessions")
        .join("sess-failed-batch")
        .join("transcript.jsonl");
    std::fs::create_dir(&transcript_path)?;

    let result = session
        .write_items(vec![compact_item("must not publish", 1)])
        .await;
    assert!(result.is_err());
    assert!(session.transcript().await.is_empty());
    assert!(session.compaction_seed().await.is_none());

    std::fs::remove_dir(&transcript_path)?;
    session
        .write_items(vec![TranscriptItem::User {
            text: "first durable item".into(),
            content: vec![],
        }])
        .await?;

    let entries = session.load_all_entries().await?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].seq, 1);
    assert!(matches!(
        &entries[0].item,
        TranscriptItem::User { text, .. } if text == "first durable item"
    ));
    Ok(())
}

#[tokio::test]
async fn concurrent_batches_receive_contiguous_non_interleaved_sequences() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-concurrent-batches".into(),
        "/tmp".into(),
        "model".into(),
        storage,
    )
    .await?;

    let left = {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .write_items(vec![
                    TranscriptItem::User {
                        text: "left-1".into(),
                        content: vec![],
                    },
                    TranscriptItem::User {
                        text: "left-2".into(),
                        content: vec![],
                    },
                ])
                .await
        })
    };
    let right = {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .write_items(vec![
                    TranscriptItem::User {
                        text: "right-1".into(),
                        content: vec![],
                    },
                    TranscriptItem::User {
                        text: "right-2".into(),
                        content: vec![],
                    },
                ])
                .await
        })
    };
    left.await??;
    right.await??;

    let entries = session.load_all_entries().await?;
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries.iter().map(|entry| entry.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    let texts = entries
        .iter()
        .filter_map(|entry| match &entry.item {
            TranscriptItem::User { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        texts == ["left-1", "left-2", "right-1", "right-2"]
            || texts == ["right-1", "right-2", "left-1", "left-2"]
    );
    Ok(())
}

#[tokio::test]
async fn independent_storage_handles_cannot_duplicate_sequences() -> TestResult {
    let dir = TempDir::new()?;
    let first_storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let created = Session::new(
        "sess-independent-handles".into(),
        "/tmp".into(),
        "model".into(),
        first_storage.clone(),
    )
    .await?;
    let second_storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let reopened = Session::open("sess-independent-handles", second_storage)
        .await?
        .ok_or_else(|| missing_error("missing reopened session"))?;

    let first = tokio::spawn(async move {
        created
            .write_items(vec![TranscriptItem::User {
                text: "first writer".into(),
                content: vec![],
            }])
            .await
    });
    let second = tokio::spawn(async move {
        reopened
            .write_items(vec![TranscriptItem::User {
                text: "second writer".into(),
                content: vec![],
            }])
            .await
    });
    first.await??;
    second.await??;

    let entries = first_storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-independent-handles".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.iter().map(|entry| entry.seq).collect::<Vec<_>>(),
        vec![1, 2]
    );
    Ok(())
}

#[tokio::test]
async fn strict_turn_write_rejects_external_advancement_without_losing_message() -> TestResult {
    let dir = TempDir::new()?;
    let first_storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let first = Session::new(
        "sess-strict-turn".into(),
        "/tmp".into(),
        "model".into(),
        first_storage.clone(),
    )
    .await?;
    let second_storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let second = Session::open("sess-strict-turn", second_storage)
        .await?
        .ok_or_else(|| missing_error("missing second session handle"))?;
    let (_, _, expected_seq) = first.context_snapshot().await;

    second
        .write_items(vec![TranscriptItem::User {
            text: "external".into(),
            content: vec![],
        }])
        .await?;
    let error = first
        .write_items_at(
            vec![TranscriptItem::User {
                text: "stale run".into(),
                content: vec![],
            }],
            expected_seq,
        )
        .await
        .err()
        .ok_or_else(|| missing_error("expected strict generation conflict"))?;
    assert!(error.to_string().contains("stale transcript write"));
    assert!(
        error
            .to_string()
            .contains("expected transcript seq 0, current seq 1"),
        "conflict should report the persisted generation: {error}"
    );

    let entries = first_storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-strict-turn".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0].item,
        TranscriptItem::User { text, .. } if text == "external"
    ));
    Ok(())
}

#[tokio::test]
async fn stale_compaction_plan_is_rejected_before_persistence() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-stale-compact".into(),
        "/tmp".into(),
        "model".into(),
        storage,
    )
    .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "planned history".into(),
            content: vec![],
        }])
        .await?;
    let expected_seq = 1;
    session
        .write_items(vec![TranscriptItem::User {
            text: "concurrent write".into(),
            content: vec![],
        }])
        .await?;

    let result = session
        .write_compact(
            compact_item("stale summary", 1),
            vec![TranscriptItem::User {
                text: "stale replacement".into(),
                content: vec![],
            }],
            expected_seq,
        )
        .await;
    assert!(result.is_err());

    let entries = session.load_all_entries().await?;
    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .all(|entry| !matches!(entry.item, TranscriptItem::Compact { .. })));
    let transcript = session.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(
        &transcript[1],
        TranscriptItem::User { text, .. } if text == "concurrent write"
    ));
    Ok(())
}

#[tokio::test]
async fn multiple_compactions_uses_last() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-multi-compact".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "msg1".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "reply1".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    // First compaction
    write_test_compact(&session, "compact-v1", vec![
        evot::compact::context_view::compact_summary_item("compact-v1"),
        TranscriptItem::User {
            text: "msg1".into(),
            content: vec![],
        },
    ])
    .await?;

    // More messages
    session
        .write_items(vec![TranscriptItem::User {
            text: "msg2".into(),
            content: vec![],
        }])
        .await?;

    // Second compaction
    write_test_compact(&session, "compact-v2", vec![
        evot::compact::context_view::compact_summary_item("compact-v2"),
        TranscriptItem::User {
            text: "msg2".into(),
            content: vec![],
        },
    ])
    .await?;

    // One more message after second compaction
    session
        .write_items(vec![TranscriptItem::User {
            text: "msg3".into(),
            content: vec![],
        }])
        .await?;

    // Load should use the second (last) compact
    let loaded = Session::open("sess-multi-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;

    // compact-v2 messages (1) + msg3 (1) = 2
    assert_eq!(transcript.len(), 3);
    assert!(
        matches!(&transcript[0], TranscriptItem::User { text, .. } if text.contains("compact-v2"))
    );
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "msg2"));
    assert!(matches!(&transcript[2], TranscriptItem::User { text, .. } if text == "msg3"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Stats filtering on resume
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stats_items_persisted_but_filtered_on_resume() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-stats".into(),
        "/tmp".into(),
        "m".into(),
        storage.clone(),
    )
    .await?;

    // Write a mix of conversation items and stats
    let stats_item =
        evot::types::TranscriptStats::LlmCallCompleted(evot::types::LlmCallCompletedStats {
            turn: 1,
            attempt: 0,
            usage: evot::types::UsageSummary {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_write: 0,
            },
            metrics: None,
            error: None,
            context_window: 0,
            stop_reason: "stop".into(),
        })
        .to_item();

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            stats_item,
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text { text: "hi".into() }],
                stop_reason: "end_turn".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;
    session.save().await?;

    // Raw storage should have 3 entries
    let raw = storage
        .list_entries(evot::types::ListTranscriptEntries {
            session_id: "sess-stats".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(raw.len(), 3);
    assert!(
        matches!(&raw[1].item, TranscriptItem::Stats { kind, .. } if kind == "llm_call_completed")
    );

    // Resumed session transcript should only have 2 items (no stats)
    let loaded = Session::open("sess-stats", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "hello"));
    assert!(
        matches!(&transcript[1], TranscriptItem::Assistant { content, ..} if matches!(&content[..], [AssistantBlock::Text { text }] if text == "hi"))
    );
    Ok(())
}

#[tokio::test]
async fn stats_after_compact_filtered_on_resume() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-stats-compact".into(),
        "/tmp".into(),
        "m".into(),
        storage.clone(),
    )
    .await?;

    // Write initial messages
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old msg".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "old reply".into(),
                }],
                stop_reason: "end_turn".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    // Write compact + stats + new message
    let compact_stats = evot::types::TranscriptStats::ContextCompactionCompleted(
        evot::types::ContextCompactionCompletedStats {
            reason: evot::types::CompactReason::Threshold,
            result: evot::types::CompactionResult::Compacted {
                before_message_count: 10,
                after_message_count: 4,
                before_tokens: 30000,
                after_tokens: 12000,
                messages_evicted: 6,
                tool_results_shrunk: 2,
                images_downgraded: 0,
                current_run_reclaimed: 0,
            },
            context_window: 0,
            will_retry: false,
        },
    )
    .to_item();

    write_test_compact(&session, "summary", vec![
        evot::compact::context_view::compact_summary_item("summary"),
    ])
    .await?;
    session
        .write_items(vec![compact_stats, TranscriptItem::User {
            text: "new msg".into(),
            content: vec![],
        }])
        .await?;
    session.save().await?;

    // Resume: should see compact base + new msg, no stats
    let loaded = Session::open("sess-stats-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(
        matches!(&transcript[0], TranscriptItem::User { text, .. } if text.contains("summary"))
    );
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "new msg"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Planning mode — user input must not be polluted by planning prompt
// ---------------------------------------------------------------------------

/// The old bug: planning prompt was prepended to user input and stored as a
/// single User transcript item. `first_user_title` then picked up the planning
/// prompt as the session title. This test reproduces the old bug scenario and
/// proves that a polluted User message yields a wrong title.
#[tokio::test]
async fn title_is_wrong_when_planning_prompt_pollutes_user_message() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-old-bug".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    // Reproduce the OLD behavior: planning prompt + user input in one message.
    let polluted = format!(
        "You are in planning mode\n\nUser task:\n{}",
        "refactor the auth module to use JWT"
    );
    session
        .write_items(vec![TranscriptItem::User {
            text: polluted,
            content: vec![],
        }])
        .await?;
    session.save().await?;

    let loaded = Session::open("sess-old-bug", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let title = loaded
        .meta()
        .await
        .title
        .ok_or_else(|| missing_error("missing title"))?;

    // Title starts with planning prompt — this is the bug we fixed.
    assert!(title.starts_with("You are in planning mode"));
    assert_ne!(title, "refactor the auth module to use JWT");
    Ok(())
}

/// After the fix, planning prompt lives in system_prompt, not in the user
/// message. When run_loop stores only the raw user input, `first_user_title`
/// derives the correct title.
#[tokio::test]
async fn title_is_correct_when_user_message_is_clean() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-plan".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    // The NEW behavior: only raw user input in the transcript.
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "refactor the auth module to use JWT".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "planning".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;
    session.save().await?;

    let loaded = Session::open("sess-plan", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing planning session"))?;
    let title = loaded
        .meta()
        .await
        .title
        .ok_or_else(|| missing_error("missing session title"))?;

    assert_eq!(title, "refactor the auth module to use JWT");
    Ok(())
}

// ---------------------------------------------------------------------------
// Marker tests — /clear, /goto, new Compact marker
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clear_marker_resets_context() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-clear".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "msg1".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "reply1".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    session.write_clear_marker().await?;

    // In-memory transcript should be empty after clear
    assert!(session.transcript().await.is_empty());

    // New messages after clear
    session
        .write_items(vec![TranscriptItem::User {
            text: "fresh start".into(),
            content: vec![],
        }])
        .await?;

    // Reload from storage — should only see post-clear messages
    let loaded = Session::open("sess-clear", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 1);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "fresh start"));
    Ok(())
}

#[tokio::test]
async fn structured_compact_entry_rebuilds_context() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-new-compact".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                content: vec![AssistantBlock::Text {
                    text: "old reply".into(),
                }],
                stop_reason: "stop".into(),
                usage: UsageSummary::default(),
                model: String::new(),
                provider: String::new(),
                timestamp: 0,
                error_message: None,
            },
        ])
        .await?;

    let replacement = vec![TranscriptItem::User {
        text: "compacted summary".into(),
        content: vec![],
    }];
    let mut item = compact_item("compacted summary", 1);
    if let TranscriptItem::Compact {
        messages,
        engine_messages,
        ..
    } = &mut item
    {
        *messages = replacement.clone();
        *engine_messages = evot::agent::run::convert::into_agent_messages(&replacement);
    }
    session.write_compact(item, replacement, 2).await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "after compact".into(),
            content: vec![],
        }])
        .await?;

    // Reload — should see the exact compact snapshot plus the new message.
    let loaded = Session::open("sess-new-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(
        matches!(&transcript[0], TranscriptItem::User { text, .. } if text.contains("compacted summary"))
    );
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "after compact"));
    Ok(())
}

#[tokio::test]
async fn opens_legacy_compact_from_its_last_active_baseline() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session_id = "sess-legacy-compact";
    storage
        .save_session(SessionMeta::new(
            session_id.into(),
            "/tmp".into(),
            "model".into(),
        ))
        .await?;
    let session_dir = dir.path().join("sessions").join(session_id);
    let lines = [
        format!(
            r#"{{"session_id":"{session_id}","run_id":null,"seq":1,"turn":0,"item":{{"type":"user","text":"discarded"}},"created_at":"2026-01-01T00:00:01Z"}}"#
        ),
        format!(
            r#"{{"session_id":"{session_id}","run_id":null,"seq":2,"turn":0,"item":{{"type":"user","text":"retained"}},"created_at":"2026-01-01T00:00:02Z"}}"#
        ),
        format!(
            r#"{{"session_id":"{session_id}","run_id":null,"seq":3,"turn":0,"item":{{"type":"assistant","text":"retained reply","tool_calls":[],"stop_reason":"stop"}},"created_at":"2026-01-01T00:00:03Z"}}"#
        ),
        format!(
            r#"{{"session_id":"{session_id}","run_id":null,"seq":4,"turn":0,"item":{{"type":"compact","id":"old-compact","created_at":1,"reason":"threshold","summary":"old summary","first_kept_seq":2,"tokens_before":100,"tokens_after":20,"messages_before":3,"messages_after":3,"details":{{}}}},"created_at":"2026-01-01T00:00:04Z"}}"#
        ),
        format!(
            r#"{{"session_id":"{session_id}","run_id":null,"seq":5,"turn":0,"item":{{"type":"user","text":"after compact"}},"created_at":"2026-01-01T00:00:05Z"}}"#
        ),
    ];
    std::fs::write(
        session_dir.join("transcript.jsonl"),
        format!("{}\n", lines.join("\n")),
    )?;

    let session = Session::open(session_id, storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing legacy compact session"))?;
    let context = session.transcript().await;
    assert_eq!(context.len(), 4);
    assert!(
        matches!(&context[0], TranscriptItem::User { text, .. } if text.contains("old summary"))
    );
    assert!(matches!(&context[1], TranscriptItem::User { text, .. } if text == "retained"));
    assert!(
        matches!(&context[2], TranscriptItem::Assistant { content, .. } if assistant_text(content) == "retained reply")
    );
    assert!(matches!(&context[3], TranscriptItem::User { text, .. } if text == "after compact"));
    let (engine_context, seed, next_seq) = session.context_snapshot().await;
    assert_eq!(engine_context.len(), 4);
    assert_eq!(next_seq, 4);
    assert!(seed.is_none());

    session
        .write_items(vec![TranscriptItem::User {
            text: "continued".into(),
            content: vec![],
        }])
        .await?;
    let raw = session.load_all_entries().await?;
    assert_eq!(raw.last().map(|entry| entry.seq), Some(5));
    Ok(())
}

#[tokio::test]
async fn opens_legacy_compact_marker_snapshot() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session_id = "sess-legacy-marker";
    storage
        .save_session(SessionMeta::new(
            session_id.into(),
            "/tmp".into(),
            "model".into(),
        ))
        .await?;
    let marker = format!(
        r#"{{"session_id":"{session_id}","run_id":null,"seq":10,"turn":0,"item":{{"type":"marker","kind":"compact","messages":[{{"type":"user","text":"snapshot"}},{{"type":"assistant","text":"snapshot reply","tool_calls":[],"stop_reason":"stop"}}]}},"created_at":"2026-01-01T00:00:10Z"}}"#
    );
    let after = format!(
        r#"{{"session_id":"{session_id}","run_id":null,"seq":11,"turn":0,"item":{{"type":"user","text":"after marker"}},"created_at":"2026-01-01T00:00:11Z"}}"#
    );
    std::fs::write(
        dir.path()
            .join("sessions")
            .join(session_id)
            .join("transcript.jsonl"),
        format!("{marker}\n{after}\n"),
    )?;

    let session = Session::open(session_id, storage)
        .await?
        .ok_or_else(|| missing_error("missing legacy marker session"))?;
    let context = session.transcript().await;
    assert_eq!(context.len(), 3);
    assert!(matches!(&context[0], TranscriptItem::User { text, .. } if text == "snapshot"));
    assert!(
        matches!(&context[1], TranscriptItem::Assistant { content, .. } if assistant_text(content) == "snapshot reply")
    );
    assert!(matches!(&context[2], TranscriptItem::User { text, .. } if text == "after marker"));
    let (engine_context, _, next_seq) = session.context_snapshot().await;
    assert_eq!(engine_context.len(), 3);
    assert_eq!(next_seq, 3);
    Ok(())
}

#[test]
fn marker_item_is_not_context() {
    let item = TranscriptItem::Marker {
        kind: evot::types::MarkerKind::Clear,
        messages: vec![],
    };
    assert!(!item.is_context_item());
}
