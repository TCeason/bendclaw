//! Core agent loop behavior: prompt → LLM stream → tool execution, abort,
//! continue-from-tool-result, error reporting, and event streaming.

use evotengine::agent_loop;
use evotengine::provider::mock::*;
use evotengine::provider::MockProvider;
use evotengine::types::AgentContext;
use evotengine::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::fixtures::agent_harness::collect_events;
use crate::fixtures::agent_harness::make_config;
use crate::fixtures::agent_harness::MockTool;
use crate::fixtures::agent_harness::TestHarness;

fn system_reminder_count(messages: &[AgentMessage]) -> usize {
    messages
        .iter()
        .filter(|message| {
            matches!(
                message,
                AgentMessage::Llm(Message::User { content, .. })
                    if content.iter().any(|content| matches!(content, Content::Text { text } if text.contains("<system-reminder>")))
            )
        })
        .count()
}

// ---------------------------------------------------------------------------
// Tests using TestHarness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simple_text_response() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello, world!".into())])
        .system_prompt("You are helpful.")
        .run("Hi")
        .await;

    output.assert_completed();
    assert!(output.has_event("TurnStart"));
    assert!(output.has_event("TurnEnd"));

    output.assert_message_count(2);
    assert_eq!(output.messages[0].role(), "user");
    assert_eq!(output.messages[1].role(), "assistant");
    assert_eq!(output.context_messages.len(), 2);
}

#[tokio::test]
async fn test_tool_call_and_response() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "read".into(),
                arguments: serde_json::json!({"path": "test.txt"}),
            }]),
            MockResponse::Text("The file contains: hello".into()),
        ])
        .tool(MockTool::ok("read", "hello"))
        .run("Read test.txt")
        .await;

    assert!(output.has_event("ToolExecStart"));
    assert!(output.has_event("ToolExecEnd"));

    // Messages: user, assistant(tool_call), toolResult, assistant(text)
    // No convergence reminder is injected — the guidance that used to live in
    // a runtime <system-reminder> now sits in the static system prompt, so
    // nothing extra gets pushed into context between tool_result and the next
    // assistant turn.
    output.assert_message_count(4);
    assert_eq!(output.messages[0].role(), "user");
    assert_eq!(output.messages[1].role(), "assistant");
    assert_eq!(output.messages[2].role(), "toolResult");
    assert_eq!(output.messages[3].role(), "assistant");
    assert_eq!(system_reminder_count(&output.messages), 0);
}

#[tokio::test]
async fn test_abort_cancels_loop() {
    // This test needs direct cancel token access — keep manual setup
    let provider = MockProvider::text("Should not appear");
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    assert!(new_messages.len() <= 2);
}

#[tokio::test]
async fn test_continue_from_tool_result() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Done processing.".into())])
        .prior_messages(vec![
            AgentMessage::Llm(Message::user("do something")),
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id: "tc-1".into(),
                tool_name: "test_tool".into(),
                content: vec![Content::Text {
                    text: "result".into(),
                }],
                is_error: false,
                timestamp: 0,
                retention: Retention::Normal,
            }),
        ])
        .run_continue()
        .await;

    assert!(!output.messages.is_empty());
    assert_eq!(output.messages[0].role(), "assistant");
}

#[tokio::test]
async fn test_tool_error_is_reported() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "failing_tool".into(),
                arguments: serde_json::json!({}),
            }]),
            MockResponse::Text("Tool failed, sorry.".into()),
        ])
        .tool(MockTool::err("failing_tool", "Something went wrong"))
        .run("Use the tool")
        .await;

    assert_eq!(output.tool_errors().len(), 1);
    output.assert_last_role("assistant");
}

#[tokio::test]
async fn test_unknown_tool_reports_error() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "nonexistent".into(),
                arguments: serde_json::json!({}),
            }]),
            MockResponse::Text("I couldn't find that tool.".into()),
        ])
        .run("Use nonexistent tool")
        .await;

    assert_eq!(output.tool_errors().len(), 1);
}

#[tokio::test]
async fn test_no_convergence_reminder_injected() {
    // Historical behaviour: the loop injected a "Continue the current user
    // request..." <system-reminder> after the first batch of tool calls.
    // That reminder turned out to train the model to open the next turn with
    // `Continue:` / `<system>继续：…` preambles copied from the wording, so
    // the injection was removed. The guidance now lives in the system prompt
    // instead.
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "read".into(),
                arguments: serde_json::json!({"path": "a.txt"}),
            }]),
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "read".into(),
                arguments: serde_json::json!({"path": "b.txt"}),
            }]),
            MockResponse::Text("Done.".into()),
        ])
        .tool(MockTool::ok("read", "hello"))
        .run("Read files")
        .await;

    assert_eq!(system_reminder_count(&output.messages), 0);
    assert_eq!(output.injected_counts(), vec![0, 0, 0]);
}

#[tokio::test]
async fn test_no_convergence_reminder_with_steering() {
    // With steering arriving from the tool channel, the loop used to skip the
    // convergence reminder. The reminder is gone entirely now, so we just
    // verify steering still flows through as a normal user message.
    struct SteeringTool {
        queue: std::sync::Arc<parking_lot::Mutex<Vec<AgentMessage>>>,
    }

    #[async_trait::async_trait]
    impl AgentTool for SteeringTool {
        fn name(&self) -> &str {
            "steering_tool"
        }
        fn label(&self) -> &str {
            "steering_tool"
        }
        fn description(&self) -> &str {
            "Tool that queues steering while executing"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            self.queue
                .lock()
                .push(AgentMessage::Llm(Message::user("stop now")));
            Ok(ToolResult {
                content: vec![Content::Text {
                    text: "hello".into(),
                }],
                details: serde_json::Value::Null,
                retention: Retention::Normal,
            })
        }
    }

    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "steering_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Handled steering.".into()),
    ]);
    let queue: std::sync::Arc<parking_lot::Mutex<Vec<AgentMessage>>> =
        std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut config = make_config(provider);
    config.get_steering_messages = {
        let queue = queue.clone();
        Some(Box::new(move || queue.lock().drain(..).collect()))
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(SteeringTool {
            queue: queue.clone(),
        })],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let prompt = AgentMessage::Llm(Message::user("Read test.txt"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);
    let output = crate::fixtures::agent_harness::TestOutput {
        messages,
        events,
        context_messages: context.messages,
    };

    assert_eq!(system_reminder_count(&output.messages), 0);
    assert!(output.messages.iter().any(|message| matches!(
        message,
        AgentMessage::Llm(Message::User { content, .. })
            if content.iter().any(|content| matches!(content, Content::Text { text } if text == "stop now"))
    )));
}
// ---------------------------------------------------------------------------
// Event streaming bug fix test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_message_update_events_emitted_during_streaming() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello, world!".into())])
        .run("hi")
        .await;

    // Collect MessageUpdate text deltas
    let deltas: Vec<String> = output
        .events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::MessageUpdate {
                delta: StreamDelta::Text { delta, .. },
                ..
            } => Some(delta.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !deltas.is_empty(),
        "Expected MessageUpdate events with text deltas, got none"
    );
    let full_text: String = deltas.into_iter().collect();
    assert_eq!(full_text, "Hello, world!");

    // Verify event ordering: MessageStart before MessageUpdate before MessageEnd
    let event_types: Vec<&str> = output
        .events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::MessageStart { .. } => Some("Start"),
            AgentEvent::MessageUpdate { .. } => Some("Update"),
            AgentEvent::MessageEnd { .. } => Some("End"),
            _ => None,
        })
        .collect();

    let assistant_start = event_types.iter().rposition(|&e| e == "Start").unwrap();
    let assistant_end = event_types.iter().rposition(|&e| e == "End").unwrap();

    for (i, &et) in event_types.iter().enumerate() {
        if et == "Update" {
            assert!(
                i > assistant_start && i < assistant_end,
                "MessageUpdate at index {} should be between MessageStart ({}) and MessageEnd ({})",
                i,
                assistant_start,
                assistant_end
            );
        }
    }
}
