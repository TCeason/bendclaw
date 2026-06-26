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

/// The anchor is the provider's real input count (uncached + cached input),
/// plus a byte estimate of only the messages appended after it.
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
    // 100_000 anchor (input + cache_read) + trailing user message:
    // 400 bytes / 4 = 100 content tokens + 4 user-envelope overhead.
    assert_eq!(estimate, 100_000 + 104);
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

    // Anchored on the real 98k, not a whole-list byte guess.
    assert_eq!(estimate, 98_000);
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
    assert_eq!(tracker.estimate_context_tokens(&fresh), 40_000);
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

// ---------------------------------------------------------------------------
// ExecutionTracker — idle clock excludes user-wait time from the duration limit
// ---------------------------------------------------------------------------

fn short_duration_limits() -> ExecutionLimits {
    ExecutionLimits {
        max_turns: 1_000_000,
        max_total_tokens: usize::MAX,
        max_duration: std::time::Duration::from_millis(50),
    }
}

/// A guard's lifetime is subtracted from elapsed, so a wait longer than the
/// duration limit does not trip `check_limits` as long as it was spent idle.
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
