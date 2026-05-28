//! Scenario-based compact tests using the compact_scenario DSL.
//!
//! Each test describes a realistic session pattern and verifies compact
//! handles it without toothpaste-squeezing or other degenerate behavior.

use fixtures::compact_scenario::*;

use super::fixtures;

// ---------------------------------------------------------------------------
// Scenario: many screenshots, then extended tool use
// ---------------------------------------------------------------------------

#[test]
fn scenario_image_heavy_then_tool_use() {
    scenario("image-heavy then tool use")
        .budget(50_000)
        .system_tokens(0)
        .keep_recent(4)
        .keep_first(1)
        .seed(vec![
            Turn::user("start the task"),
            Turn::user_image("screenshot 1", "/tmp/s1.png"),
            Turn::assistant("I see the UI"),
            Turn::user_image("screenshot 2", "/tmp/s2.png"),
            Turn::assistant("Got it"),
            Turn::user_image("screenshot 3", "/tmp/s3.png"),
            Turn::assistant("Noted"),
            Turn::user_image("screenshot 4", "/tmp/s4.png"),
            Turn::assistant("OK"),
            Turn::user_image("screenshot 5", "/tmp/s5.png"),
            Turn::assistant("Understood"),
            Turn::user_image("screenshot 6", "/tmp/s6.png"),
            Turn::assistant("I see"),
            Turn::user_image("screenshot 7", "/tmp/s7.png"),
            Turn::assistant("Ready"),
            Turn::user_image("screenshot 8", "/tmp/s8.png"),
            Turn::assistant("Let me work on this"),
        ])
        .phase(15, Turn::tool("bash", 200))
        .assert_no_toothpaste()
        .assert_images_stripped_when_pressured()
        .run();
}

// ---------------------------------------------------------------------------
// Scenario: long session with steady tool use (no images)
// ---------------------------------------------------------------------------

#[test]
fn scenario_long_tool_session() {
    scenario("long tool session")
        .budget(20_000)
        .system_tokens(0)
        .keep_recent(6)
        .keep_first(1)
        .max_messages(150)
        .seed(vec![Turn::user("build the project")])
        .phase(60, Turn::tool("bash", 300))
        .assert_no_toothpaste()
        .assert_message_count_below(150)
        .run();
}

// ---------------------------------------------------------------------------
// Scenario: tool overhead doesn't cause false pressure
// ---------------------------------------------------------------------------

#[test]
fn scenario_tool_overhead_no_false_pressure() {
    // 50k budget, 20k tool overhead. Messages stay under trigger.
    // Compact should never fire uselessly.
    scenario("tool overhead ghost pressure")
        .budget(50_000)
        .system_tokens(0)
        .tool_overhead(20_000)
        .keep_recent(6)
        .keep_first(1)
        .seed(vec![Turn::user("start")])
        .phase(20, Turn::tool("bash", 200))
        .assert_no_toothpaste()
        .run();
}

// ---------------------------------------------------------------------------
// Scenario: mixed images + large tool outputs
// ---------------------------------------------------------------------------

#[test]
fn scenario_mixed_images_and_large_outputs() {
    // 5 images (27k) + large tool outputs push past trigger.
    // budget/4 = 10k, image_tokens = 27k > 10k triggers strip.
    scenario("mixed images and large outputs")
        .budget(40_000)
        .system_tokens(0)
        .keep_recent(6)
        .keep_first(1)
        .seed(vec![
            Turn::user("analyze this"),
            Turn::user_image("page 1", "/tmp/p1.png"),
            Turn::assistant("got page 1"),
            Turn::user_image("page 2", "/tmp/p2.png"),
            Turn::assistant("got page 2"),
            Turn::user_image("page 3", "/tmp/p3.png"),
            Turn::assistant("got page 3"),
            Turn::user_image("page 4", "/tmp/p4.png"),
            Turn::assistant("got page 4"),
            Turn::user_image("page 5", "/tmp/p5.png"),
            Turn::assistant("got page 5"),
        ])
        .phase(10, Turn::tool("bash", 2000))
        .assert_no_toothpaste()
        .assert_images_stripped_when_pressured()
        .run();
}

// ---------------------------------------------------------------------------
// Scenario: rapid small turns (message count pressure)
// ---------------------------------------------------------------------------

#[test]
fn scenario_rapid_small_turns() {
    // 100 tool turns = 201 messages. max_messages=386 (default) won't evict.
    // Token pressure from small outputs will trigger shrink/microcompact.
    scenario("rapid small turns")
        .budget(5_000)
        .system_tokens(0)
        .keep_recent(8)
        .keep_first(1)
        .seed(vec![Turn::user("quick tasks")])
        .phase(100, Turn::tool("bash", 50))
        .assert_no_toothpaste()
        .run();
}

// ---------------------------------------------------------------------------
// Rewrites of existing multi-round tests using DSL
// ---------------------------------------------------------------------------

/// Equivalent to test_multi_round_compaction_prevents_toothpaste_squeezing
#[test]
fn scenario_60_turn_tool_session_compacts() {
    scenario("60-turn tool session")
        .budget(3_000)
        .system_tokens(0)
        .keep_recent(10)
        .keep_first(2)
        .seed(vec![Turn::user("start the task")])
        .phase(60, Turn::tool("bash", 250))
        .assert_compaction_fires()
        .assert_message_count_below(121) // less than theoretical max (1 + 60*2)
        .run();
}

/// Equivalent to test_multi_round_l2_escalation
#[test]
fn scenario_l2_escalation_from_large_user_messages() {
    scenario("L2 escalation from large user messages")
        .budget(800)
        .system_tokens(0)
        .keep_recent(2)
        .keep_first(1)
        .max_messages(8)
        .seed(vec![Turn::user("start")])
        .phase(20, Turn::user_large("question", "context ", 80))
        .assert_drops_messages()
        .assert_final_within_budget()
        .run();
}

/// Equivalent to test_multi_round_repeated_l2_escalation
#[test]
fn scenario_repeated_l2_with_large_keep_recent() {
    scenario("repeated L2 with large keep_recent")
        .budget(3_000)
        .system_tokens(0)
        .keep_recent(20)
        .keep_first(1)
        .seed(vec![Turn::user("start")])
        .phase(
            40,
            Turn::user_large(
                "question about architecture",
                "the quick brown fox jumps over the lazy dog. ",
                20,
            ),
        )
        .assert_drops_messages()
        .assert_final_within_budget()
        .assert_message_count_below(30)
        .run();
}
