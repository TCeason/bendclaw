//! Steering behavior: user messages injected mid-run and their accounting
//! in `LlmCallStart`.

use evotengine::provider::mock::*;
use evotengine::*;

use crate::fixtures::agent_harness::TestHarness;

// ---------------------------------------------------------------------------
// Steering tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_steering_messages_injected_into_context() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("I see your steering.".into())])
        .steering(vec![AgentMessage::Llm(Message::user("change direction"))])
        .run("Hi")
        .await;

    output.assert_completed();

    // Context should contain: steering msg + user prompt + assistant response
    let user_msgs: Vec<_> = output
        .context_messages
        .iter()
        .filter(|m| m.role() == "user")
        .collect();
    assert_eq!(
        user_msgs.len(),
        2,
        "Expected steering + prompt user messages"
    );
}

#[tokio::test]
async fn test_steering_count_reported_in_llm_call_start() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Got it.".into())])
        .steering(vec![
            AgentMessage::Llm(Message::user("steer 1")),
            AgentMessage::Llm(Message::user("steer 2")),
        ])
        .run("Hi")
        .await;

    output.assert_completed();

    let counts = output.injected_counts();
    assert!(!counts.is_empty(), "Expected at least one LlmCallStart");
    assert_eq!(counts[0], 2, "Expected 2 injected messages");
}

#[tokio::test]
async fn test_no_steering_reports_zero() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello.".into())])
        .run("Hi")
        .await;

    output.assert_completed();

    let counts = output.injected_counts();
    assert!(!counts.is_empty());
    assert_eq!(counts[0], 0, "Expected 0 injected messages");
}
