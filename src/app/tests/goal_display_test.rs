//! Tests for `agent::goal::display` formatting helpers.

use evot::agent::goal::display::format_show;
use evot::agent::goal::display::format_summary;
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
    assert!(out.contains("not complete yet"));
    assert!(out.contains("pending"));
}

#[test]
fn format_show_active_includes_last_verification() {
    let mut goal = SessionGoal::new("refactor storage".into(), GoalBudget::default());
    goal.progress.iterations = 1;
    goal.progress.last_reason = Some("tests still fail".into());

    let out = format_show(&goal);
    assert!(out.contains("not complete"));
    assert!(out.contains("Last verification: not complete"));
    assert!(out.contains("tests still fail"));
}

#[test]
fn format_show_met_is_explicitly_complete() {
    let mut goal = SessionGoal::new("refactor storage".into(), GoalBudget::default());
    goal.status = GoalStatus::Met;
    goal.progress.last_reason = Some("all checks passed".into());

    let out = format_show(&goal);
    assert!(out.contains("Goal complete"));
    assert!(out.contains("all checks passed"));
}
