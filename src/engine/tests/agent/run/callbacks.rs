//! Lifecycle callback behavior: `before_turn` aborts, `after_turn` usage
//! reporting, error events, and optional callbacks.

use evotengine::agent_loop;
use evotengine::provider::mock::*;
use evotengine::provider::MockProvider;
use evotengine::provider::ProviderError;
use evotengine::types::AgentContext;
use evotengine::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::common::FailThenSucceedProvider;
use super::common::ProgressTool;
use crate::fixtures::agent_harness::collect_events;
use crate::fixtures::agent_harness::make_config;
use crate::fixtures::agent_harness::TestHarness;

// ---------------------------------------------------------------------------
// Lifecycle callback tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_before_turn_can_abort() {
    // Provider with 5 text responses, but before_turn aborts after 2 turns.
    // We need tool calls to keep the loop going for multiple turns.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        // These should never be reached
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Final".into()),
    ]);

    let turn_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let turn_count_clone = turn_count.clone();

    let mut config = make_config(provider);
    config.before_turn = Some(std::sync::Arc::new(move |_msgs, _turn| {
        let count = turn_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        count < 2 // Allow turns 0 and 1, abort on turn 2
    }));

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // before_turn was called 3 times (allowed 0, allowed 1, rejected 2)
    assert_eq!(turn_count.load(std::sync::atomic::Ordering::SeqCst), 3);

    // Only 2 assistant messages should be produced
    let assistant_count = new_messages
        .iter()
        .filter(|m| m.role() == "assistant")
        .count();
    assert_eq!(assistant_count, 2);
}

#[tokio::test]
async fn test_after_turn_receives_messages() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Done.".into()),
    ]);

    let message_counts: std::sync::Arc<std::sync::Mutex<Vec<usize>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let counts_clone = message_counts.clone();

    let mut config = make_config(provider);
    config.after_turn = Some(std::sync::Arc::new(move |msgs, _usage| {
        counts_clone.lock().unwrap().push(msgs.len());
    }));

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let counts = message_counts.lock().unwrap();
    // after_turn called twice (one per LLM response)
    assert_eq!(counts.len(), 2);
    // Message count should increase between calls
    assert!(counts[1] > counts[0], "counts: {:?}", *counts);
}

#[tokio::test]
async fn test_error_event_fires_on_provider_error() {
    let provider = FailThenSucceedProvider {
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        max_failures: 10, // more failures than retries
        error: ProviderError::Network("connection reset".into()),
        inner: MockProvider::text("never reached"),
    };

    let mut config = make_config(MockProvider::text("unused"));
    config.provider = std::sync::Arc::new(provider);
    config.retry_policy = evotengine::RetryPolicy::disabled();

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);
    let error_events: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Error { error } => Some(error.message.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(error_events.len(), 1);
    assert!(
        error_events[0].contains("connection reset"),
        "got: {}",
        error_events[0]
    );
}

#[tokio::test]
async fn test_callbacks_are_optional() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello!".into())])
        .run("Hi")
        .await;

    output.assert_message_count(2);
    output.assert_completed();
}
