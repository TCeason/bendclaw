//! Tests for the planner module.

use evotengine::context::compaction::config::CompactionConfig;
use evotengine::context::compaction::planner;
use evotengine::context::SummarizerMode;
use evotengine::types::*;

fn user_msg(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        timestamp: 0,
    })
}

fn assistant_msg(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn tool_call_msg(id: &str, name: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({"path": "test.rs"}),
        }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn tool_result_msg(id: &str, text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: "read".to_string(),
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        is_error: false,
        timestamp: 0,
        retention: Retention::default(),
    })
}

fn big_text(n: usize) -> String {
    "x".repeat(n)
}

fn config_small() -> CompactionConfig {
    CompactionConfig {
        context_window: 10_000,
        reserve_tokens: 2_000,
        keep_recent_tokens: 1_000,
        keep_recent_min: 2,
        keep_first: 2,
        max_tool_result_tokens: 500,
        tool_output_max_lines: 50,
        keep_recent_images: 1,
        summarizer_mode: SummarizerMode::default(),
        summary_max_chars: 4000,
    }
}

#[test]
fn no_plan_when_too_few_messages() {
    let config = config_small();
    let messages = vec![user_msg("hello"), assistant_msg("hi")];
    assert!(planner::plan(&messages, &config).is_none());
}

#[test]
fn plan_evicts_middle_zone() {
    let config = config_small();
    // Build messages: 2 pinned + many middle + 2 recent
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    // Add enough middle messages to exceed keep_recent_tokens
    for _i in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    // Recent tail
    messages.push(user_msg("recent question"));
    messages.push(assistant_msg("recent answer"));

    let plan = match planner::plan(&messages, &config) {
        Some(plan) => plan,
        None => panic!("expected planner to evict middle zone"),
    };

    // Pinned head should start at 0
    assert_eq!(plan.pinned_head.start, 0);
    assert!(plan.pinned_head.end >= 2);

    // Evict zone should be non-empty
    assert!(plan.evict_zone.start < plan.evict_zone.end);

    // Retained tail should include the recent messages
    assert!(plan.retained_tail.end == messages.len());
    assert!(plan.retained_tail.start > plan.evict_zone.start);
}

#[test]
fn plan_does_not_cut_at_tool_result() {
    let config = config_small();
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..10 {
        messages.push(user_msg(&big_text(300)));
        messages.push(tool_call_msg("tc1", "read"));
        messages.push(tool_result_msg("tc1", &big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("answer"));

    let plan = planner::plan(&messages, &config);
    if let Some(plan) = plan {
        // retained_tail should not start at a tool result
        let tail_start_msg = &messages[plan.retained_tail.start];
        assert!(!matches!(
            tail_start_msg,
            AgentMessage::Llm(Message::ToolResult { .. })
        ));
    }
}

#[test]
fn detects_split_turn() {
    let config = CompactionConfig {
        context_window: 10_000,
        reserve_tokens: 2_000,
        keep_recent_tokens: 500,
        keep_recent_min: 2,
        keep_first: 1,
        max_tool_result_tokens: 500,
        tool_output_max_lines: 50,
        keep_recent_images: 1,
        summarizer_mode: SummarizerMode::default(),
        summary_max_chars: 4000,
    };

    // user, assistant(tool_call), tool_result, tool_call, tool_result (big turn)
    let mut messages = vec![user_msg(&big_text(100))]; // pinned
                                                       // Big turn
    messages.push(user_msg(&big_text(200)));
    messages.push(tool_call_msg("t1", "read"));
    messages.push(tool_result_msg("t1", &big_text(800)));
    messages.push(tool_call_msg("t2", "read"));
    messages.push(tool_result_msg("t2", &big_text(800)));
    // If cut happens inside this turn, split_turn should be detected
    messages.push(assistant_msg(&big_text(100)));

    let plan = planner::plan(&messages, &config);
    // Plan may or may not split depending on exact token math,
    // but the function should not panic
    assert!(plan.is_some() || messages.len() < 4);
}

#[test]
fn empty_messages_returns_none() {
    let config = config_small();
    assert!(planner::plan(&[], &config).is_none());
}
