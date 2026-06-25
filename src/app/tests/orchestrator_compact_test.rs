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
use evot::compact::orchestrator::CompactSettings;
use evot::compact::orchestrator::CompactSummarizer;
use evot::compact::orchestrator::ManualCompactRequest;
use evot::conf::LlmConfig;
use evot::conf::Protocol;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
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
    }
}

fn summarizer(provider: Arc<CapturingProvider>) -> CompactSummarizer {
    CompactSummarizer {
        provider,
        llm: test_llm(),
        max_tokens: 4096,
    }
}

fn settings() -> CompactSettings {
    CompactSettings {
        keep_recent_tokens: KEEP_RECENT_TOKENS,
        keep_recent_min_messages: 2,
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
    compact_session(
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
