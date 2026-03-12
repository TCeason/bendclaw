use std::time::Duration;
use std::time::Instant;

use bendclaw::kernel::run::run_loop::LLMResponse;
use bendclaw::kernel::run::run_loop::RunLoopConfig;
use bendclaw::kernel::run::run_loop::RunLoopState;
use bendclaw::kernel::run::transition::apply_turn_result;
use bendclaw::kernel::run::transition::TurnTransition;
use bendclaw::kernel::run::Reason;
use bendclaw::kernel::Message;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::usage::TokenUsage;

fn run_loop_state() -> RunLoopState {
    RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(60),
            max_context_tokens: 8192,
        },
        Instant::now(),
    )
}

fn final_turn() -> LLMResponse {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ThinkingDelta("reason".to_string()));
    turn.apply_stream_event(StreamEvent::ContentDelta("final answer".to_string()));
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(7, 3)));
    turn.set_ttft_ms(15);
    turn
}

fn tool_turn() -> LLMResponse {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ContentDelta("use tool".to_string()));
    turn.apply_stream_event(StreamEvent::ToolCallEnd {
        index: 0,
        id: "tc-1".to_string(),
        name: "shell".to_string(),
        arguments: r#"{"command":"ls"}"#.to_string(),
    });
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(10, 5)));
    turn.set_ttft_ms(42);
    turn
}

#[test]
fn apply_turn_result_records_error_and_returns_error_transition() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = final_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        Some("boom"),
        None,
        "mock-model",
        Duration::from_secs(60),
    );

    assert_eq!(transition, TurnTransition::Error(Reason::Error));
    assert_eq!(messages.len(), 2);
    assert!(matches!(
        &messages[0],
        Message::OperationEvent { kind, name, status, .. }
            if kind == "llm" && name == "reasoning.turn" && status == "failed"
    ));
    assert!(matches!(
        &messages[1],
        Message::Error { message, .. } if message == "boom"
    ));
    assert!(!state.should_continue());
}

#[test]
fn apply_turn_result_returns_done_and_records_final_content() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = final_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        None,
        "mock-model",
        Duration::from_secs(60),
    );

    assert_eq!(transition, TurnTransition::Done);
    assert_eq!(messages.len(), 1);
    assert!(!state.should_continue());
    assert_eq!(state.final_content().len(), 2);
}

#[test]
fn apply_turn_result_returns_dispatch_for_tool_turn() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = tool_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        None,
        "mock-model",
        Duration::from_secs(60),
    );

    assert_eq!(transition, TurnTransition::DispatchTools);
    assert_eq!(messages.len(), 1);
    assert!(state.should_continue());
    assert!(matches!(
        &messages[0],
        Message::Assistant { tool_calls, .. } if tool_calls.len() == 1
    ));
}

#[test]
fn apply_turn_result_returns_abort_and_appends_aborted_tool_results() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = tool_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        Some(Reason::Timeout),
        "mock-model",
        Duration::from_secs(60),
    );

    assert_eq!(transition, TurnTransition::Abort(Reason::Timeout));
    assert_eq!(messages.len(), 2);
    assert!(matches!(
        &messages[1],
        Message::ToolResult { output, success, .. } if output == "aborted" && !success
    ));
    assert!(state.should_continue());
}
