use evot::agent::session::Session;
use evot::compact::context_view::resolve_context_items;
use evot::compact::orchestrator::compact_session;
use evot::compact::orchestrator::CompactSettings;
use evot::compact::orchestrator::ManualCompactRequest;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use evot::types::CompactReason;
use evot::types::TranscriptItem;
use evot::types::UsageSummary;
use tempfile::TempDir;

const KEEP_RECENT_TOKENS: usize = 1;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn compact_session_persists_structured_item_with_summary_override() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-compact-orchestrator".into(),
        "/tmp".into(),
        "m".into(),
        storage,
    )
    .await?;

    session
        .write_items(vec![
            user("old one with enough text to summarize"),
            assistant("old assistant response"),
            user("recent request"),
            assistant("recent answer"),
        ])
        .await?;

    let compact = compact_session(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Threshold,
            custom_instructions: None,
            summary_override: Some("LLM supplied summary".into()),
            summarizer: None,
            settings: CompactSettings {
                keep_recent_tokens: KEEP_RECENT_TOKENS,
                keep_recent_min_messages: 2,
            },
        },
        tokio_util::sync::CancellationToken::new(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("expected compaction"))?;

    let TranscriptItem::Compact {
        summary,
        first_kept_seq,
        reason,
        ..
    } = compact
    else {
        return Err(std::io::Error::other("expected compact item").into());
    };

    assert_eq!(summary, "LLM supplied summary");
    assert_eq!(reason, CompactReason::Threshold);
    assert!(first_kept_seq > 1);

    let raw = session.load_all_entries().await?;
    assert!(matches!(
        raw.last().map(|e| &e.item),
        Some(TranscriptItem::Compact { .. })
    ));

    let context = session.transcript().await;
    assert!(
        matches!(&context[0], TranscriptItem::User { text, .. } if text.contains("LLM supplied summary"))
    );
    assert!(context
        .iter()
        .any(|item| matches!(item, TranscriptItem::User { text, .. } if text == "recent request")));
    assert!(!context.iter().any(
        |item| matches!(item, TranscriptItem::User { text, .. } if text.starts_with("old one"))
    ));

    Ok(())
}

#[tokio::test]
async fn compact_context_view_uses_latest_compact_boundary() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-compact-boundary".into(),
        "/tmp".into(),
        "m".into(),
        storage,
    )
    .await?;

    session
        .write_items(vec![
            user("old"),
            assistant("old reply"),
            user("kept"),
            assistant("kept reply"),
        ])
        .await?;
    compact_session(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Manual,
            custom_instructions: None,
            summary_override: Some("summary one".into()),
            summarizer: None,
            settings: CompactSettings {
                keep_recent_tokens: KEEP_RECENT_TOKENS,
                keep_recent_min_messages: 2,
            },
        },
        tokio_util::sync::CancellationToken::new(),
    )
    .await?;
    session.write_items(vec![user("after")]).await?;

    let entries = session.load_all_entries().await?;
    let context = resolve_context_items(&entries);

    assert!(
        matches!(&context[0], TranscriptItem::User { text, .. } if text.contains("summary one"))
    );
    assert!(context
        .iter()
        .any(|item| matches!(item, TranscriptItem::User { text, .. } if text == "kept")));
    assert!(context
        .iter()
        .any(|item| matches!(item, TranscriptItem::User { text, .. } if text == "after")));
    assert!(!context
        .iter()
        .any(|item| matches!(item, TranscriptItem::User { text, .. } if text == "old")));

    Ok(())
}

fn user(text: &str) -> TranscriptItem {
    TranscriptItem::User {
        text: text.into(),
        content: vec![],
    }
}

fn assistant(text: &str) -> TranscriptItem {
    TranscriptItem::Assistant {
        text: text.into(),
        thinking: None,
        tool_calls: vec![],
        stop_reason: "stop".into(),
        usage: UsageSummary::default(),
        model: String::new(),
        provider: String::new(),
        timestamp: 0,
        error_message: None,
    }
}
