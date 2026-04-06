use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::conf::LlmConfig;
use bendclaw::conf::ProviderKind;
use bendclaw::conf::StorageConfig;
use bendclaw::error::Result;
use bendclaw::request::*;
use bendclaw::storage::model::ListRunEvents;
use bendclaw::storage::model::ListTranscriptEntries;
use bendclaw::storage::model::RunEvent;
use bendclaw::storage::model::RunEventKind;
use bendclaw::storage::model::RunMeta;
use bendclaw::storage::model::RunStatus;
use bendclaw::storage::model::TranscriptKind;
use bendclaw::storage::open_storage;
use tempfile::TempDir;
use tokio::sync::Mutex;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn is_uuid_v7(value: &str) -> bool {
    match uuid::Uuid::parse_str(value) {
        Ok(value) => value.get_version_num() == 7,
        Err(_) => false,
    }
}

fn fs_store(root: &TempDir) -> StorageConfig {
    StorageConfig::fs(root.path().to_path_buf())
}

fn test_llm_config() -> LlmConfig {
    LlmConfig {
        provider: ProviderKind::Anthropic,
        api_key: "test-key".into(),
        base_url: None,
        model: "claude-sonnet-4-20250514".into(),
    }
}

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

fn make_assistant_message(text: &str) -> bend_engine::AgentMessage {
    bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
        content: vec![bend_engine::Content::Text { text: text.into() }],
        stop_reason: bend_engine::StopReason::Stop,
        model: "claude".into(),
        provider: "anthropic".into(),
        usage: bend_engine::Usage::default(),
        timestamp: 0,
        error_message: None,
    })
}

struct CollectSink {
    events: Mutex<Vec<Arc<RunEvent>>>,
}

impl CollectSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    async fn events(&self) -> Vec<Arc<RunEvent>> {
        self.events.lock().await.clone()
    }
}

#[async_trait]
impl EventSink for CollectSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

#[tokio::test]
async fn full_pipeline_creates_session_and_run() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;
    let sink = Arc::new(CollectSink::new());

    let final_messages = vec![
        bend_engine::AgentMessage::Llm(bend_engine::Message::user("hello")),
        make_assistant_message("hi there"),
    ];

    let assistant_msg = final_messages[1].clone();
    let agent_events = vec![
        bend_engine::AgentEvent::TurnStart,
        bend_engine::AgentEvent::MessageEnd {
            message: assistant_msg,
        },
        bend_engine::AgentEvent::AgentEnd {
            messages: final_messages.clone(),
        },
    ];

    let runner = RequestRunner::scripted(agent_events, final_messages);
    let request = Request::new("hello".into());

    RequestExecutor::new(
        request,
        test_llm_config(),
        sink.clone(),
        storage.clone(),
        runner,
    )
    .execute()
    .await?;

    let events = sink.events().await;
    assert!(events.len() >= 4);

    let kinds: Vec<_> = events.iter().map(|event| &event.kind).collect();
    assert!(matches!(kinds[0], RunEventKind::RunStarted));
    assert!(matches!(kinds[1], RunEventKind::TurnStarted));
    assert!(matches!(kinds[2], RunEventKind::AssistantCompleted));
    assert!(matches!(kinds[3], RunEventKind::RunFinished));

    let session_id = &events[0].session_id;
    let run_id = &events[0].run_id;

    assert!(is_uuid_v7(session_id));
    assert!(is_uuid_v7(run_id));
    assert!(is_uuid_v7(&events[0].event_id));

    let session_meta = storage
        .get_session(session_id)
        .await?
        .ok_or_else(|| missing_error("missing session meta"))?;
    assert_eq!(session_meta.session_id, *session_id);

    let transcript = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: session_id.clone(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(transcript.len(), 2);

    let run_events = storage
        .list_run_events(ListRunEvents {
            run_id: run_id.clone(),
        })
        .await?;
    assert_eq!(run_events.len(), 4);

    Ok(())
}

#[tokio::test]
async fn pipeline_marks_failed_when_no_result() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;
    let sink = Arc::new(CollectSink::new());

    // No AgentEnd → got_agent_end stays false → run marked Failed
    let agent_events = vec![bend_engine::AgentEvent::InputRejected {
        reason: "api failed".into(),
    }];

    let runner = RequestRunner::scripted(agent_events, vec![]);
    let request = Request::new("hello".into());

    RequestExecutor::new(
        request,
        test_llm_config(),
        sink.clone(),
        storage.clone(),
        runner,
    )
    .execute()
    .await?;

    let events = sink.events().await;
    let run_id = &events[0].run_id;
    let session_id = &events[0].session_id;
    let meta_path = root
        .path()
        .join("sessions")
        .join(session_id)
        .join("runs")
        .join(format!("{run_id}.json"));
    let content = std::fs::read_to_string(meta_path)?;
    let run_meta: RunMeta = serde_json::from_str(&content)?;
    assert_eq!(run_meta.status, RunStatus::Failed);

    Ok(())
}

#[tokio::test]
async fn pipeline_resume_session() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;

    let first_messages = vec![
        bend_engine::AgentMessage::Llm(bend_engine::Message::user("hello")),
        make_assistant_message("hi"),
    ];

    let first_events = vec![
        bend_engine::AgentEvent::TurnStart,
        bend_engine::AgentEvent::MessageEnd {
            message: first_messages[1].clone(),
        },
        bend_engine::AgentEvent::AgentEnd {
            messages: first_messages.clone(),
        },
    ];

    let runner1 = RequestRunner::scripted(first_events, first_messages.clone());
    let sink1 = Arc::new(CollectSink::new());

    RequestExecutor::new(
        Request::new("hello".into()),
        test_llm_config(),
        sink1.clone(),
        storage.clone(),
        runner1,
    )
    .execute()
    .await?;

    let session_id = sink1
        .events()
        .await
        .first()
        .ok_or_else(|| missing_error("missing first event"))?
        .session_id
        .clone();

    let second_messages = vec![
        bend_engine::AgentMessage::Llm(bend_engine::Message::user("hello")),
        make_assistant_message("hi"),
        bend_engine::AgentMessage::Llm(bend_engine::Message::user("continue")),
        make_assistant_message("ok"),
    ];

    let second_events = vec![
        bend_engine::AgentEvent::TurnStart,
        bend_engine::AgentEvent::MessageEnd {
            message: second_messages[3].clone(),
        },
        bend_engine::AgentEvent::AgentEnd {
            messages: second_messages.clone(),
        },
    ];

    let runner2 = RequestRunner::scripted(second_events, second_messages.clone());
    let sink2 = Arc::new(CollectSink::new());
    let mut request = Request::new("continue".into());
    request.session_id = Some(session_id.clone());

    RequestExecutor::new(
        request,
        test_llm_config(),
        sink2.clone(),
        storage.clone(),
        runner2,
    )
    .execute()
    .await?;

    let transcript = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: session_id.clone(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(transcript.len(), 4);

    let kinds: Vec<_> = transcript.iter().map(|e| &e.kind).collect();
    assert!(matches!(kinds[0], TranscriptKind::User));
    assert!(matches!(kinds[1], TranscriptKind::Assistant));
    assert!(matches!(kinds[2], TranscriptKind::User));
    assert!(matches!(kinds[3], TranscriptKind::Assistant));

    Ok(())
}

#[test]
fn request_started_event_has_correct_kind() {
    let event = request_started_event("run-001", "sess-001");
    assert!(matches!(event.kind, RunEventKind::RunStarted));
    assert_eq!(event.turn, 0);
}
