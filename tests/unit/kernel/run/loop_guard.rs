use bendclaw::kernel::run::loop_guard::LoopGuard;
use bendclaw::kernel::run::loop_guard::LoopGuardConfig;
use bendclaw::kernel::run::loop_guard::LoopGuardVerdict;
use serde_json::json;

fn guard(max_identical: u32, window: usize, breaker: u32) -> LoopGuard {
    LoopGuard::new(LoopGuardConfig {
        max_identical_calls: max_identical,
        window_size: window,
        circuit_breaker: breaker,
    })
}

#[test]
fn allows_first_call() {
    let mut g = LoopGuard::default();
    let v = g.check("shell", &json!({"command": "ls"}));
    assert_eq!(v, LoopGuardVerdict::Allow);
}

#[test]
fn blocks_after_max_identical_calls() {
    let mut g = guard(3, 10, 20);
    let args = json!({"command": "ls"});

    g.record("shell", &args);
    g.record("shell", &args);
    g.record("shell", &args);

    let v = g.check("shell", &args);
    assert!(matches!(v, LoopGuardVerdict::Block(_)));
}

#[test]
fn warns_on_penultimate_call() {
    let mut g = guard(3, 10, 20);
    let args = json!({"command": "ls"});

    g.record("shell", &args);
    g.record("shell", &args);

    // 3rd call = max_identical_calls, should warn
    let v = g.check("shell", &args);
    assert!(matches!(v, LoopGuardVerdict::Warn(_)));
}

#[test]
fn different_args_are_independent() {
    let mut g = guard(2, 10, 20);

    g.record("shell", &json!({"command": "ls"}));
    g.record("shell", &json!({"command": "pwd"}));

    let v = g.check("shell", &json!({"command": "ls"}));
    assert!(matches!(v, LoopGuardVerdict::Warn(_)));

    let v = g.check("shell", &json!({"command": "cat"}));
    assert_eq!(v, LoopGuardVerdict::Allow);
}

#[test]
fn window_slides_old_calls_fall_off() {
    let mut g = guard(3, 4, 20);
    let args = json!({"x": 1});

    // Fill window with 3 identical calls
    g.record("tool", &args);
    g.record("tool", &args);
    g.record("tool", &args);

    // Push 4 different calls to fully flush the window
    for _ in 0..4 {
        g.record("other", &json!({}));
    }

    // All original calls have fallen off the window
    let v = g.check("tool", &args);
    assert_eq!(v, LoopGuardVerdict::Allow);
}

#[test]
fn circuit_breaker_trips_after_enough_blocks() {
    let mut g = guard(1, 10, 2);

    // First block
    g.record("a", &json!({}));
    let v = g.check("a", &json!({}));
    assert!(matches!(v, LoopGuardVerdict::Block(_)));

    // Second block (different tool)
    g.record("b", &json!({}));
    let v = g.check("b", &json!({}));
    assert!(matches!(v, LoopGuardVerdict::Block(_)));

    // Circuit breaker tripped — even a new tool is blocked
    let v = g.check("c", &json!({"new": true}));
    assert!(matches!(v, LoopGuardVerdict::Block(ref msg) if msg.contains("circuit breaker")));
}

#[test]
fn json_key_order_does_not_matter() {
    let mut g = guard(2, 10, 20);

    g.record("tool", &json!({"b": 2, "a": 1}));

    // Same keys, different order
    let v = g.check("tool", &json!({"a": 1, "b": 2}));
    assert!(matches!(v, LoopGuardVerdict::Warn(_)));
}
