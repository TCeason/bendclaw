//! Tests for `ContextTracker` — context size measured from the provider's own
//! usage embedded in the transcript, not a local tokenizer.

use evotengine::context::ContextTracker;
use evotengine::context::ExecutionLimits;
use evotengine::context::ExecutionTracker;
use evotengine::context::IdleClock;
use evotengine::types::*;

fn user_msg(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::User {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        timestamp: 0,
    })
}

fn assistant_with_input(text: &str, input: u64, cache_read: u64) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage {
            input,
            output: 50,
            cache_read,
            cache_write: 0,
            total_tokens: input + cache_read + 50,
            reasoning_output: 0,
        },
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

/// The anchor is the provider's real total usage, plus a byte estimate of only
/// the messages appended after it.
#[test]
fn anchors_on_latest_assistant_usage_plus_trailing() {
    let tracker = ContextTracker::new();
    let trailing = "x".repeat(400); // ~100 tokens at len/4
    let messages = vec![
        user_msg("hello"),
        assistant_with_input("hi", 90_000, 10_000),
        user_msg(&trailing),
    ];

    let estimate = tracker.estimate_context_tokens(&messages);
    // 100_050 provider total (input + cache_read + output), plus the trailing
    // user message: 400 bytes / 4 = 100 content tokens + 4 envelope tokens.
    assert_eq!(estimate, 100_050 + 104);
}

/// The fix for the resume bug: a fresh tracker (as built on every resumed run)
/// still reports the provider-anchored size because the anchor lives in the
/// message list, not in lost in-memory state.
#[test]
fn fresh_tracker_recovers_anchor_on_resume() {
    let messages = vec![
        user_msg("earlier turn"),
        assistant_with_input("answer", 98_000, 0),
    ];

    // A brand-new tracker is what a resumed session starts with.
    let resumed = ContextTracker::new();
    let estimate = resumed.estimate_context_tokens(&messages);

    // Anchored on the real provider total, including 50 output tokens.
    assert_eq!(estimate, 98_050);
}

/// Before any assistant response (first turn of a fresh session) there is no
/// anchor, so the byte estimate over the whole list is the floor.
#[test]
fn falls_back_to_byte_estimate_without_anchor() {
    let tracker = ContextTracker::new();
    let messages = vec![user_msg(&"x".repeat(400))];

    let estimate = tracker.estimate_context_tokens(&messages);
    // No provider usage yet: ~100 bytes/4 + envelope, far from any anchor.
    assert!(estimate > 0 && estimate < 1_000);
}

/// After compaction, the trailing assistant usage reflects the pre-compaction
/// (larger) context and must be suppressed until a fresh response lands.
#[test]
fn suppresses_stale_anchor_after_compaction() {
    let mut tracker = ContextTracker::new();
    // Pre-compaction response measured a huge context.
    let messages = vec![
        user_msg("q"),
        assistant_with_input("big-context answer", 150_000, 0),
    ];

    tracker.record_compaction_done();
    // Compaction shrank the list; the stale 150k anchor must not be used.
    let after_compaction = tracker.estimate_context_tokens(&messages);
    assert!(
        after_compaction < 1_000,
        "stale anchor should be suppressed, got {after_compaction}"
    );

    // A genuine new response re-enables anchoring.
    tracker.record_response(&Usage {
        input: 40_000,
        output: 10,
        cache_read: 0,
        cache_write: 0,
        total_tokens: 40_010,
        reasoning_output: 0,
    });
    let fresh = vec![
        user_msg("q"),
        assistant_with_input("post-compaction answer", 40_000, 0),
    ];
    assert_eq!(tracker.estimate_context_tokens(&fresh), 40_050);
}

/// An empty or error response carries no usable usage and must not clear the
/// post-compaction suppression — otherwise the stale anchor would resurface.
#[test]
fn empty_response_does_not_reenable_stale_anchor() {
    let mut tracker = ContextTracker::new();
    tracker.record_compaction_done();
    tracker.record_response(&Usage::default()); // empty: input/cache all zero

    let messages = vec![user_msg("q"), assistant_with_input("stale", 150_000, 0)];
    let estimate = tracker.estimate_context_tokens(&messages);
    assert!(
        estimate < 1_000,
        "empty response must not revive the stale anchor, got {estimate}"
    );
}

#[test]
fn output_only_usage_is_not_a_context_anchor() {
    let tracker = ContextTracker::new();
    let large_history = "x".repeat(40_000);
    let mut output_only = assistant_with_input("synthetic", 0, 0);
    if let AgentMessage::Llm(Message::Assistant { usage, .. }) = &mut output_only {
        usage.output = 10;
        usage.total_tokens = 10;
    }
    let messages = vec![user_msg(&large_history), output_only, user_msg("trailing")];

    let estimate = tracker.estimate_context_tokens(&messages);
    assert!(
        estimate > 9_000,
        "output-only usage must not collapse a large context estimate: {estimate}"
    );
}

#[test]
fn output_only_response_does_not_reenable_stale_anchor() {
    let mut tracker = ContextTracker::new();
    tracker.record_compaction_done();
    tracker.record_response(&Usage {
        output: 10,
        total_tokens: 10,
        ..Default::default()
    });
    let messages = vec![user_msg("q"), assistant_with_input("stale", 150_000, 0)];

    assert!(tracker.estimate_context_tokens(&messages) < 1_000);
}

#[test]
fn model_switch_does_not_reuse_foreign_usage_anchor() {
    let tracker = ContextTracker::new();
    let large_history = "x".repeat(40_000);
    let mut old_model = assistant_with_input("old answer", 100, 0);
    if let AgentMessage::Llm(Message::Assistant { model, .. }) = &mut old_model {
        *model = "old-model".into();
    }
    let messages = vec![user_msg(&large_history), old_model, user_msg("next prompt")];

    let old_estimate =
        tracker.estimate_context_tokens_for_model(&messages, Some("test"), Some("old-model"));
    let new_estimate =
        tracker.estimate_context_tokens_for_model(&messages, Some("test"), Some("new-model"));

    assert!(old_estimate < 1_000, "matching model should use its anchor");
    assert!(
        new_estimate > 9_000,
        "new model must estimate the complete history instead of reusing foreign usage: {new_estimate}"
    );
}

#[test]
fn provider_switch_does_not_reuse_same_named_model_anchor() {
    let tracker = ContextTracker::new();
    let large_history = "x".repeat(40_000);
    let mut foreign_provider = assistant_with_input("old answer", 100, 0);
    if let AgentMessage::Llm(Message::Assistant {
        provider, model, ..
    }) = &mut foreign_provider
    {
        *provider = "old-provider".into();
        *model = "shared-model".into();
    }
    let messages = vec![user_msg(&large_history), foreign_provider];

    let estimate = tracker.estimate_context_tokens_for_model(
        &messages,
        Some("new-provider"),
        Some("shared-model"),
    );

    assert!(
        estimate > 9_000,
        "same model id from another provider must not anchor context: {estimate}"
    );
}

#[test]
fn native_total_tokens_take_precedence_over_component_fallback() {
    let tracker = ContextTracker::new();
    let mut assistant = assistant_with_input("answer", 90_000, 10_000);
    if let AgentMessage::Llm(Message::Assistant { usage, .. }) = &mut assistant {
        usage.total_tokens = 100_123;
    }

    assert_eq!(tracker.estimate_context_tokens(&[assistant]), 100_123);
}

#[test]
fn zero_native_total_falls_back_to_usage_components() {
    let tracker = ContextTracker::new();
    let mut assistant = assistant_with_input("answer", 90_000, 10_000);
    if let AgentMessage::Llm(Message::Assistant { usage, .. }) = &mut assistant {
        usage.total_tokens = 0;
    }

    assert_eq!(tracker.estimate_context_tokens(&[assistant]), 100_050);
}

// ---------------------------------------------------------------------------
// ExecutionTracker — idle clock excludes tool execution time from the duration
// limit. The loop pauses the clock around every tool call, so `max_duration`
// bounds only the agent's own work (LLM inference + loop overhead), never how
// long a build, a training run, or a slow user reply takes.
// ---------------------------------------------------------------------------

fn short_duration_limits() -> ExecutionLimits {
    ExecutionLimits {
        max_turns: 1_000_000,
        max_total_tokens: usize::MAX,
        max_duration: std::time::Duration::from_millis(50),
    }
}

/// A guard's lifetime is subtracted from elapsed, so a tool (or user wait)
/// longer than the duration limit does not trip `check_limits` as long as it
/// ran inside a pause interval.
#[test]
fn idle_time_is_excluded_from_duration_limit() {
    let clock = IdleClock::new();
    let tracker = ExecutionTracker::with_idle_clock(short_duration_limits(), clock.clone());

    // Simulate a user taking far longer than the 50ms limit to answer.
    {
        let _pause = clock.pause();
        std::thread::sleep(std::time::Duration::from_millis(120));
    }

    assert!(
        tracker.check_limits().is_none(),
        "idle wait must not count toward the duration limit"
    );
}

/// Active (non-idle) time beyond the limit still trips the duration check, even
/// when an earlier idle interval was excluded.
#[test]
fn active_time_still_trips_duration_limit() {
    let clock = IdleClock::new();
    let tracker = ExecutionTracker::with_idle_clock(short_duration_limits(), clock.clone());

    {
        let _pause = clock.pause();
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    // Active work that on its own exceeds the 50ms limit.
    std::thread::sleep(std::time::Duration::from_millis(60));

    match tracker.check_limits() {
        Some(reason) => assert!(
            reason.contains("Max duration"),
            "expected duration limit, got: {reason}"
        ),
        None => panic!("active time beyond the limit should trip the duration check"),
    }
}

/// The idle clock accumulates across multiple separate waits.
#[test]
fn idle_intervals_accumulate() {
    let clock = IdleClock::new();
    {
        let _p = clock.pause();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    {
        let _p = clock.pause();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        clock.accumulated() >= std::time::Duration::from_millis(35),
        "two ~20ms waits should accumulate to ~40ms, got {:?}",
        clock.accumulated()
    );
}
