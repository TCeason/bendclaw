//! Tests for `agent::goal::policy::decide`.

use evot::agent::goal::policy;
use evot::agent::goal::policy::Decision;
use evot::agent::goal::EvalVerdict;
use evot::types::GoalBudget;
use evot::types::GoalStatus;
use evot::types::SessionGoal;

fn goal_with_status(status: GoalStatus) -> SessionGoal {
    let mut g = SessionGoal::new("ship the feature".into(), GoalBudget::default());
    g.status = status;
    g
}

#[test]
fn active_goal_with_continue_verdict_continues() {
    let goal = goal_with_status(GoalStatus::Active);
    let verdict = EvalVerdict::Continue;
    match policy::decide(&goal, Some(&verdict)) {
        Decision::Continue { prompt } => assert!(!prompt.is_empty()),
        _ => panic!("expected Continue"),
    }
}

#[test]
fn active_goal_with_no_verdict_continues() {
    let goal = goal_with_status(GoalStatus::Active);
    match policy::decide(&goal, None) {
        Decision::Continue { prompt } => assert!(!prompt.is_empty()),
        _ => panic!("expected Continue when no verdict"),
    }
}

#[test]
fn active_goal_with_met_verdict_returns_met() {
    let goal = goal_with_status(GoalStatus::Active);
    let verdict = EvalVerdict::Met {
        reasoning: "all tests pass".into(),
    };
    match policy::decide(&goal, Some(&verdict)) {
        Decision::Met { wrap_prompt } => assert!(!wrap_prompt.is_empty()),
        _ => panic!("expected Met"),
    }
}

#[test]
fn active_goal_with_impossible_verdict_returns_impossible() {
    let goal = goal_with_status(GoalStatus::Active);
    let verdict = EvalVerdict::Impossible {
        reasoning: "dep missing".into(),
    };
    match policy::decide(&goal, Some(&verdict)) {
        Decision::Impossible { wrap_prompt } => assert!(!wrap_prompt.is_empty()),
        _ => panic!("expected Impossible"),
    }
}

#[test]
fn paused_goal_stops() {
    let goal = goal_with_status(GoalStatus::Paused);
    assert!(matches!(policy::decide(&goal, None), Decision::Stop));
}

#[test]
fn met_goal_stops() {
    let goal = goal_with_status(GoalStatus::Met);
    assert!(matches!(policy::decide(&goal, None), Decision::Stop));
}

#[test]
fn impossible_goal_stops() {
    let goal = goal_with_status(GoalStatus::Impossible);
    assert!(matches!(policy::decide(&goal, None), Decision::Stop));
}

#[test]
fn exhausted_goal_stops() {
    let goal = goal_with_status(GoalStatus::Exhausted);
    assert!(matches!(policy::decide(&goal, None), Decision::Stop));
}

#[test]
fn budget_exhausted_returns_exhausted_decision() {
    let mut goal = SessionGoal::new("do something".into(), GoalBudget {
        max_tokens: Some(1000),
        max_iterations: None,
        max_seconds: None,
    });
    goal.progress.tokens_used = 1000;
    match policy::decide(&goal, Some(&EvalVerdict::Continue)) {
        Decision::Exhausted { wrap_prompt } => assert!(!wrap_prompt.is_empty()),
        _ => panic!("expected Exhausted decision"),
    }
}
