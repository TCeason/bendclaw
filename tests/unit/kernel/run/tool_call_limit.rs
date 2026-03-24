use bendclaw::kernel::run::tool_call_limit::ToolCallLimitTracker;

#[test]
fn tracks_remaining_budget() {
    let mut tracker = ToolCallLimitTracker::new(5);
    assert_eq!(tracker.remaining(), 5);
    tracker.increment(3);
    assert_eq!(tracker.count(), 3);
    assert_eq!(tracker.remaining(), 2);
    assert!(!tracker.is_exceeded());
}

#[test]
fn limit_zero_blocks_immediately() {
    let tracker = ToolCallLimitTracker::new(0);
    assert_eq!(tracker.remaining(), 0);
    assert!(tracker.is_exceeded());
}

#[test]
fn saturates_after_limit() {
    let mut tracker = ToolCallLimitTracker::new(2);
    tracker.increment(5);
    assert_eq!(tracker.count(), 5);
    assert_eq!(tracker.remaining(), 0);
    assert!(tracker.is_exceeded());
}
