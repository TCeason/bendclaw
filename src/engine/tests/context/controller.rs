//! Tests for the CompactionController.

use evotengine::context::compaction::config::CompactionConfig;
use evotengine::context::compaction::controller::CompactionController;
use evotengine::context::compaction::types::AfterResponseAction;
use evotengine::context::compaction::types::ModelId;
use evotengine::context::compaction::types::UsageSnapshot;
use evotengine::context::SummarizerMode;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;

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
        stop_reason: evotengine::StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn big_text(n: usize) -> String {
    "x".repeat(n)
}

fn model_id() -> ModelId {
    ModelId {
        provider: "test".into(),
        model: "test".into(),
    }
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

#[tokio::test]
async fn controller_skips_when_below_threshold() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg("hello"), assistant_msg("hi")];

    let usage = UsageSnapshot {
        input: 500,
        cache_read: 0,
        cache_write: 0,
        output: 100,
        model: model_id(),
        timestamp: 1000,
        stop_reason: StopReason::Stop,
        error_message: None,
    };

    let cancel = CancellationToken::new();
    let response = ctrl
        .after_response(&mut messages, &usage, &model_id(), None, cancel)
        .await;
    assert_eq!(response.action, AfterResponseAction::Continue);
    assert!(response.stats.is_none());
    assert_eq!(messages.len(), 2); // unchanged
}

#[tokio::test]
async fn controller_compacts_on_threshold() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let original_count = messages.len();

    // Usage that exceeds threshold (10_000 - 2_000 = 8_000)
    let usage = UsageSnapshot {
        input: 8_500,
        cache_read: 0,
        cache_write: 0,
        output: 500,
        model: model_id(),
        timestamp: 1000,
        stop_reason: StopReason::Stop,
        error_message: None,
    };

    let cancel = CancellationToken::new();
    let response = ctrl
        .after_response(&mut messages, &usage, &model_id(), None, cancel)
        .await;
    assert_eq!(response.action, AfterResponseAction::Continue);
    assert!(response.stats.is_some());
    assert!(messages.len() < original_count);
}

#[tokio::test]
async fn controller_retries_on_overflow() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    // This is the error message that will be removed
    messages.push(assistant_msg("error response"));

    let original_count = messages.len();

    let usage = UsageSnapshot {
        input: 0,
        cache_read: 0,
        cache_write: 0,
        output: 0,
        model: model_id(),
        timestamp: 1000,
        stop_reason: StopReason::Error,
        error_message: Some("prompt is too long: 50000 tokens > 10000 maximum".into()),
    };

    let cancel = CancellationToken::new();
    let response = ctrl
        .after_response(&mut messages, &usage, &model_id(), None, cancel)
        .await;
    assert_eq!(response.action, AfterResponseAction::Retry);
    // Error message should have been popped
    assert!(messages.len() < original_count);
}

#[tokio::test]
async fn controller_does_not_retry_twice() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("error"));

    let usage = UsageSnapshot {
        input: 0,
        cache_read: 0,
        cache_write: 0,
        output: 0,
        model: model_id(),
        timestamp: 1000,
        stop_reason: StopReason::Error,
        error_message: Some("prompt is too long: 50000 tokens > 10000 maximum".into()),
    };

    // First overflow triggers retry
    let cancel = CancellationToken::new();
    let response = ctrl
        .after_response(&mut messages, &usage, &model_id(), None, cancel)
        .await;
    assert_eq!(response.action, AfterResponseAction::Retry);

    // Add another error message
    messages.push(assistant_msg("error again"));
    let usage2 = UsageSnapshot {
        input: 0,
        cache_read: 0,
        cache_write: 0,
        output: 0,
        model: model_id(),
        timestamp: 2000,
        stop_reason: StopReason::Error,
        error_message: Some("prompt is too long: 50000 tokens > 10000 maximum".into()),
    };

    // Second overflow should NOT retry
    let cancel2 = CancellationToken::new();
    let response2 = ctrl
        .after_response(&mut messages, &usage2, &model_id(), None, cancel2)
        .await;
    assert_eq!(response2.action, AfterResponseAction::Continue);
    assert!(response2.stats.is_none());
}

#[tokio::test]
async fn controller_accumulates_state_across_compactions() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    // First compaction
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let cancel = CancellationToken::new();
    ctrl.force_compact(&mut messages, None, cancel).await;
    let gen1 = match ctrl.state() {
        Some(state) => state.generation,
        None => panic!("expected state after first compaction"),
    };

    // Add more messages and compact again
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }

    let cancel2 = CancellationToken::new();
    ctrl.force_compact(&mut messages, None, cancel2).await;
    let gen2 = match ctrl.state() {
        Some(state) => state.generation,
        None => panic!("expected state after second compaction"),
    };
    assert!(gen2 > gen1);
}
