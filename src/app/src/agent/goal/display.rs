//! Goal display — formatting for `/goal show`.

use crate::types::GoalStatus;
use crate::types::SessionGoal;

/// Format the goal for `/goal show` output.
pub fn format_show(goal: &SessionGoal) -> String {
    let last_reason = goal
        .progress
        .last_reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty());

    match goal.status {
        GoalStatus::Active => {
            let turns = turns_label(goal.progress.iterations);
            let last_check = last_reason
                .map(|r| format!("\nLast verification: not complete — {r}"))
                .unwrap_or_else(|| "\nLast verification: pending".to_string());
            format!(
                "Goal active (not complete yet): {} ({turns}){last_check}",
                goal.condition
            )
        }
        GoalStatus::Paused => format!("Goal paused (not complete yet): {}", goal.condition),
        GoalStatus::Met => {
            let reason = last_reason
                .map(|r| format!("\nCompletion reason: {r}"))
                .unwrap_or_default();
            format!("Goal complete: {}{reason}", goal.condition)
        }
        GoalStatus::Impossible => {
            let reason = last_reason
                .map(|r| format!("\nLast verification: {r}"))
                .unwrap_or_default();
            format!("Goal could not be achieved: {}{reason}", goal.condition)
        }
        GoalStatus::Exhausted => format!("Goal exhausted before completion: {}", goal.condition),
    }
}

fn turns_label(iterations: u32) -> String {
    match iterations {
        0 => "not yet verified".to_string(),
        1 => "1 turn".to_string(),
        n => format!("{n} turns"),
    }
}

/// Short one-line summary for command feedback (e.g. after `/goal set`).
pub fn format_summary(goal: &SessionGoal) -> String {
    let mut parts = vec![format!("\"{}\"", goal.condition)];
    if let Some(m) = goal.budget.max_iterations {
        parts.push(format!("{m} iterations"));
    }
    if let Some(m) = goal.budget.max_tokens {
        parts.push(format!("{m} tokens"));
    }
    if let Some(m) = goal.budget.max_seconds {
        parts.push(format!("{m}s"));
    }
    parts.join(" | ")
}
