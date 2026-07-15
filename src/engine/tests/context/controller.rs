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
    assistant_msg_with_usage(text, 0, 0)
}

fn assistant_msg_with_usage(text: &str, input: u64, output: u64) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        stop_reason: evotengine::StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            total_tokens: input + output,
            reasoning_output: 0,
        },
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
        total_tokens: 0,
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
        total_tokens: 0,
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
        total_tokens: 0,
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
        total_tokens: 0,
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
        total_tokens: 0,
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
async fn controller_allows_multiple_stateless_compactions() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("recent answer"));

    let first = ctrl
        .force_compact(&mut messages, None, CancellationToken::new())
        .await;
    assert!(first.is_some());

    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }

    let second = ctrl
        .force_compact(&mut messages, None, CancellationToken::new())
        .await;
    assert!(second.is_some());
}

#[tokio::test]
async fn overflow_exhausted_signals_after_second_overflow() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(user_msg("recent"));
    messages.push(assistant_msg("error"));

    // The first overflow's compaction records last_compaction_ts = now_ms().
    // The second usage must carry a timestamp after that, otherwise it would
    // be skipped as stale rather than treated as overflow-exhausted.
    let future_ts = evotengine::context::now_ms() + 60_000;
    let overflow_usage = |ts: u64| UsageSnapshot {
        input: 0,
        cache_read: 0,
        cache_write: 0,
        output: 0,
        total_tokens: 0,
        model: model_id(),
        timestamp: ts,
        stop_reason: StopReason::Error,
        error_message: Some("prompt is too long: 50000 tokens > 10000 maximum".into()),
    };

    // First overflow triggers a compact-and-retry.
    let first = ctrl
        .after_response(
            &mut messages,
            &overflow_usage(future_ts),
            &model_id(),
            None,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(first.action, AfterResponseAction::Retry);
    assert!(!first.overflow_exhausted);

    // Second overflow this turn: recovery is exhausted. Do not retry, and
    // signal the loop to surface a user-visible message.
    messages.push(assistant_msg("error again"));
    let second = ctrl
        .after_response(
            &mut messages,
            &overflow_usage(future_ts + 1),
            &model_id(),
            None,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(second.action, AfterResponseAction::Continue);
    assert!(second.overflow_exhausted);
    assert!(second.stats.is_none());
}

#[tokio::test]
async fn estimate_compaction_does_not_reset_overflow_recovery() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);
    let overflow_usage = UsageSnapshot {
        input: 0,
        cache_read: 0,
        cache_write: 0,
        output: 0,
        total_tokens: 0,
        model: model_id(),
        timestamp: evotengine::context::now_ms() + 60_000,
        stop_reason: StopReason::Error,
        error_message: Some("prompt is too long: 50000 tokens > 10000 maximum".into()),
    };
    let mut messages = vec![user_msg(&big_text(200)), assistant_msg(&big_text(200))];
    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    messages.push(assistant_msg("first overflow"));

    let first = ctrl
        .after_response(
            &mut messages,
            &overflow_usage,
            &model_id(),
            None,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(first.action, AfterResponseAction::Retry);

    for _ in 0..20 {
        messages.push(user_msg(&big_text(300)));
        messages.push(assistant_msg(&big_text(300)));
    }
    let _ = ctrl
        .compact_on_estimate(&mut messages, 9_000, None, CancellationToken::new())
        .await;
    messages.push(assistant_msg("second overflow"));

    let second = ctrl
        .after_response(
            &mut messages,
            &UsageSnapshot {
                timestamp: overflow_usage.timestamp + 1,
                ..overflow_usage
            },
            &model_id(),
            None,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(second.action, AfterResponseAction::Continue);
    assert!(second.overflow_exhausted);
}

#[tokio::test]
async fn compact_on_estimate_compacts_when_over_threshold() {
    // Mirrors the post-response fallback for a non-overflow error: no usable
    // usage, so the controller compacts purely on the supplied estimate.
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

    // Estimate over the 8_000 threshold (window 10_000 - reserve 2_000).
    let response = ctrl
        .compact_on_estimate(&mut messages, 9_000, None, CancellationToken::new())
        .await;

    assert_eq!(response.action, AfterResponseAction::Continue);
    assert!(response.stats.is_some());
    assert!(!response.overflow_exhausted);
    assert!(messages.len() < original_count);
}

#[tokio::test]
async fn compact_on_estimate_skips_below_threshold() {
    let config = config_small();
    let mut ctrl = CompactionController::new(config);

    let mut messages = vec![user_msg("hello"), assistant_msg("hi")];
    let original_count = messages.len();

    let response = ctrl
        .compact_on_estimate(&mut messages, 1_000, None, CancellationToken::new())
        .await;

    assert_eq!(response.action, AfterResponseAction::Continue);
    assert!(response.stats.is_none());
    assert_eq!(messages.len(), original_count);
}
