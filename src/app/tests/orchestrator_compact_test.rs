//! Orchestrator-level compaction tests that exercise the LLM summarizer path.
//!
//! These cover behavior the engine summarizer tests cannot reach on their own:
//! - custom instructions are injected into the summarization prompt
//! - a prior compaction's summary is passed as `previous-summary` on the next pass
//!
//! A `CapturingProvider` records every `StreamConfig` so we can assert on the
//! prompt text actually sent to the model.

use std::sync::Arc;

use evot::agent::session::Session;
use evot::compact::orchestrator::compact_session;
use evot::compact::orchestrator::compact_session_with_status;
use evot::compact::orchestrator::CompactSessionStatus;
use evot::compact::orchestrator::CompactSettings;
use evot::compact::orchestrator::CompactSummarizer;
use evot::compact::orchestrator::ManualCompactRequest;
use evot::conf::LlmConfig;
use evot::conf::Protocol;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use evot::types::AssistantBlock;
use evot::types::CompactReason;
use evot::types::TranscriptItem;
use evot::types::UsageSummary;
use evot_engine::provider::error::ProviderError;
use evot_engine::provider::traits::StreamConfig;
use evot_engine::provider::traits::StreamEvent;
use evot_engine::provider::StreamOutcome;
use evot_engine::provider::StreamProvider;
use evot_engine::types::*;
use parking_lot::Mutex;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

const KEEP_RECENT_TOKENS: usize = 1;

struct FailingProvider;

#[async_trait::async_trait]
impl StreamProvider for FailingProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        _cancel: CancellationToken,
    ) -> std::result::Result<StreamOutcome, ProviderError> {
        Err(ProviderError::Api("summary unavailable".into()))
    }
}

struct BlockingProvider {
    started: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl StreamProvider for BlockingProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> std::result::Result<StreamOutcome, ProviderError> {
        self.started.notify_waiters();
        cancel.cancelled().await;
        Err(ProviderError::Cancelled)
    }
}

/// Provider that records requests and replies with scripted text, in order.
struct CapturingProvider {
    replies: Mutex<std::collections::VecDeque<String>>,
    captured: Arc<Mutex<Vec<StreamConfig>>>,
}

impl CapturingProvider {
    fn new(replies: Vec<&str>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().map(String::from).collect()),
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn captured(&self) -> Arc<Mutex<Vec<StreamConfig>>> {
        self.captured.clone()
    }
}

#[async_trait::async_trait]
impl StreamProvider for CapturingProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        _cancel: CancellationToken,
    ) -> std::result::Result<StreamOutcome, ProviderError> {
        self.captured.lock().push(config);
        let text = self
            .replies
            .lock()
            .pop_front()
            .unwrap_or_else(|| "FALLBACK SUMMARY".into());

        let _ = tx.send(StreamEvent::Start);
        let _ = tx.send(StreamEvent::TextDelta {
            content_index: 0,
            delta: text.clone(),
        });
        let message = Message::Assistant {
            content: vec![Content::Text { text }],
            stop_reason: StopReason::Stop,
            model: "capture".into(),
            provider: "capture".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        };
        let _ = tx.send(StreamEvent::Done {
            message: message.clone(),
        });
        Ok(StreamOutcome::complete(message))
    }
}

fn test_llm() -> LlmConfig {
    LlmConfig {
        provider: "test".into(),
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "http://localhost".into(),
        model: "gpt-4o".into(),
        thinking_level: ThinkingLevel::Off,
        compat_caps: Default::default(),
        context_window: None,
        max_tokens: None,
        supports_image: None,
    }
}

fn summarizer(provider: Arc<CapturingProvider>) -> CompactSummarizer {
    CompactSummarizer {
        provider,
        llm: test_llm(),
        max_tokens: 4096,
        timeout: std::time::Duration::from_secs(60),
    }
}

fn settings() -> CompactSettings {
    CompactSettings {
        keep_recent_tokens: KEEP_RECENT_TOKENS,
        keep_recent_min_messages: 2,
        context_window: 0,
    }
}

async fn new_session() -> std::result::Result<Arc<Session>, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    // Leak the TempDir for the duration of the test so storage stays valid.
    let path = dir.keep();
    let storage = open_storage(&StorageConfig::fs(path))?;
    let session = Session::new("sess-orch".into(), "/tmp".into(), "m".into(), storage).await?;
    Ok(session)
}

#[tokio::test]
async fn failed_llm_compaction_reports_deterministic_fallback() -> TestResult {
    let session = new_session().await?;
    session
        .write_items(vec![
            user("old request with enough content to summarize"),
            assistant("old assistant reply"),
            user("recent request"),
            assistant("recent answer"),
        ])
        .await?;

    let result = compact_session_with_status(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Manual,
            custom_instructions: None,
            summary_override: None,
            summarizer: Some(CompactSummarizer {
                provider: Arc::new(FailingProvider),
                llm: test_llm(),
                max_tokens: 4096,
                timeout: std::time::Duration::from_secs(60),
            }),
            settings: settings(),
        },
        CancellationToken::new(),
    )
    .await?;

    assert_eq!(result.status, CompactSessionStatus::Compacted);
    assert!(result.used_fallback);
    assert!(matches!(result.item, Some(TranscriptItem::Compact { .. })));
    Ok(())
}

#[tokio::test]
async fn timed_out_llm_compaction_uses_deterministic_fallback() -> TestResult {
    let session = new_session().await?;
    session
        .write_items(vec![
            user("old request with enough content to summarize"),
            assistant("old assistant reply"),
            user("recent request"),
            assistant("recent answer"),
        ])
        .await?;

    let started = Arc::new(tokio::sync::Notify::new());
    let result = compact_session_with_status(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Manual,
            custom_instructions: None,
            summary_override: None,
            summarizer: Some(CompactSummarizer {
                provider: Arc::new(BlockingProvider { started }),
                llm: test_llm(),
                max_tokens: 4096,
                timeout: std::time::Duration::from_millis(10),
            }),
            settings: settings(),
        },
        CancellationToken::new(),
    )
    .await?;

    assert_eq!(result.status, CompactSessionStatus::Compacted);
    assert!(result.used_fallback);
    assert!(matches!(result.item, Some(TranscriptItem::Compact { .. })));
    Ok(())
}

#[tokio::test]
async fn cancelled_llm_compaction_does_not_write_a_marker() -> TestResult {
    let session = new_session().await?;
    session
        .write_items(vec![
            user("old request with enough content to summarize"),
            assistant("old assistant reply"),
            user("recent request"),
            assistant("recent answer"),
        ])
        .await?;

    let started = Arc::new(tokio::sync::Notify::new());
    let cancel = CancellationToken::new();
    let compact_session = session.clone();
    let compact_cancel = cancel.clone();
    let provider = Arc::new(BlockingProvider {
        started: started.clone(),
    });
    let task = tokio::spawn(async move {
        compact_session_with_status(
            &compact_session,
            ManualCompactRequest {
                reason: CompactReason::Manual,
                custom_instructions: None,
                summary_override: None,
                summarizer: Some(CompactSummarizer {
                    provider,
                    llm: test_llm(),
                    max_tokens: 4096,
                    timeout: std::time::Duration::from_secs(60),
                }),
                settings: settings(),
            },
            compact_cancel,
        )
        .await
    });

    started.notified().await;
    cancel.cancel();
    let result = task.await??;
    assert_eq!(result.status, CompactSessionStatus::Cancelled);
    assert!(result.item.is_none());
    assert!(!session
        .load_all_entries()
        .await?
        .iter()
        .any(|entry| matches!(entry.item, TranscriptItem::Compact { .. })));
    Ok(())
}

#[tokio::test]
async fn llm_compaction_injects_custom_instructions() -> TestResult {
    let session = new_session().await?;
    session
        .write_items(vec![
            user("old request with enough content to summarize"),
            assistant("old assistant reply"),
            user("recent request"),
            assistant("recent answer"),
        ])
        .await?;

    let provider = Arc::new(CapturingProvider::new(vec!["LLM SUMMARY"]));
    let captured = provider.captured();

    compact_session(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Manual,
            custom_instructions: Some("focus on architectural decisions".into()),
            summary_override: None,
            summarizer: Some(summarizer(provider)),
            settings: settings(),
        },
        CancellationToken::new(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("expected compaction"))?;

    let prompt = first_user_prompt(&captured)?;
    assert!(
        prompt.contains("Additional user instructions"),
        "missing instructions header in prompt: {prompt}"
    );
    assert!(prompt.contains("focus on architectural decisions"));
    Ok(())
}

#[tokio::test]
async fn second_compaction_passes_previous_summary() -> TestResult {
    let session = new_session().await?;
    session
        .write_items(vec![
            user("first old request with content"),
            assistant("first old reply"),
            user("kept request"),
            assistant("kept reply"),
        ])
        .await?;

    // First compaction produces a summary via the LLM.
    let provider1 = Arc::new(CapturingProvider::new(vec!["FIRST PASS SUMMARY"]));
    compact_session(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Manual,
            custom_instructions: None,
            summary_override: None,
            summarizer: Some(summarizer(provider1)),
            settings: settings(),
        },
        CancellationToken::new(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("expected first compaction"))?;

    // Add more turns, then compact again.
    session
        .write_items(vec![
            user("second batch request with content"),
            assistant("second batch reply"),
            user("newest request"),
            assistant("newest answer"),
        ])
        .await?;

    let provider2 = Arc::new(CapturingProvider::new(vec!["SECOND PASS SUMMARY"]));
    let captured2 = provider2.captured();
    let second = compact_session(
        &session,
        ManualCompactRequest {
            reason: CompactReason::Manual,
            custom_instructions: None,
            summary_override: None,
            summarizer: Some(summarizer(provider2)),
            settings: settings(),
        },
        CancellationToken::new(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("expected second compaction"))?;

    let prompt = first_user_prompt(&captured2)?;
    assert!(
        prompt.contains("<previous-summary>"),
        "second pass should embed previous summary: {prompt}"
    );
    assert!(prompt.contains("FIRST PASS SUMMARY"));
    let TranscriptItem::Compact {
        state,
        messages,
        engine_messages,
        ..
    } = second
    else {
        return Err(std::io::Error::other("expected structured compact item").into());
    };
    assert_eq!(state.generation, 2);
    assert_eq!(state.last_summary.as_deref(), Some("SECOND PASS SUMMARY"));
    assert!(!messages.is_empty());
    assert_eq!(engine_messages.len(), messages.len());
    assert!(messages
        .iter()
        .any(|item| matches!(item, TranscriptItem::User { text, .. } if text == "newest request")));
    Ok(())
}

fn first_user_prompt(
    captured: &Arc<Mutex<Vec<StreamConfig>>>,
) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let requests = captured.lock();
    let config = requests
        .first()
        .ok_or_else(|| std::io::Error::other("no captured request"))?;
    let text = match config.messages.first() {
        Some(Message::User { content, .. }) => content
            .iter()
            .filter_map(|c| match c {
                Content::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>(),
        _ => String::new(),
    };
    Ok(text)
}

fn user(text: &str) -> TranscriptItem {
    TranscriptItem::User {
        text: text.into(),
        content: vec![],
    }
}

fn assistant(text: &str) -> TranscriptItem {
    TranscriptItem::Assistant {
        content: vec![AssistantBlock::Text { text: text.into() }],
        stop_reason: "stop".into(),
        usage: UsageSummary::default(),
        model: String::new(),
        provider: String::new(),
        timestamp: 0,
        error_message: None,
    }
}
