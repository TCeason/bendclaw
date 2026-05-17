//! Tests for `agent::goal::display` formatting helpers.

use evot::agent::goal::display::format_show;
use evot::agent::goal::display::format_summary;
use evot::agent::goal::display::format_system_prompt_block;
use evot::types::GoalBudget;
use evot::types::GoalStatus;
use evot::types::SessionGoal;

#[test]
fn format_summary_no_budget() {
    let goal = SessionGoal::new("plain condition".into(), GoalBudget {
        max_tokens: None,
        max_iterations: None,
        max_seconds: None,
    });
    let out = format_summary(&goal);
    assert!(out.contains("plain condition"));
}

#[test]
fn format_summary_with_budget() {
    let goal = SessionGoal::new("plan".into(), GoalBudget {
        max_tokens: Some(100_000),
        max_iterations: Some(10),
        max_seconds: None,
    });
    let out = format_summary(&goal);
    assert!(out.contains("plan"));
    assert!(out.contains("10 iterations"));
    assert!(out.contains("100000 tokens"));
}

#[test]
fn format_show_includes_condition_and_status() {
    let goal = SessionGoal::new("refactor storage".into(), GoalBudget::default());
    let out = format_show(&goal);
    assert!(out.contains("refactor storage"));
    assert!(out.contains("active"));
}

#[test]
fn system_prompt_block_none_for_terminal() {
    let mut goal = SessionGoal::new("ship".into(), GoalBudget::default());
    goal.status = GoalStatus::Met;
    assert!(format_system_prompt_block(&goal).is_none());
    goal.status = GoalStatus::Paused;
    assert!(format_system_prompt_block(&goal).is_none());
}

#[test]
fn system_prompt_block_none_for_active() {
    let goal = SessionGoal::new("ship the refactor".into(), GoalBudget::default());
    assert!(format_system_prompt_block(&goal).is_none());
}
