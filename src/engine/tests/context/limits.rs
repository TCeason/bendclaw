//! Tests for `ExecutionTracker` / `IdleClock` — idle time (tool execution,
//! user waits) is excluded from the duration limit, so `max_duration` bounds
//! only the agent's own work.

use evotengine::context::ExecutionLimits;
use evotengine::context::ExecutionTracker;
use evotengine::context::IdleClock;

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
