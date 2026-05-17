//! Goal display — formatting for `/goal show` and system prompt injection.

use crate::types::GoalStatus;
use crate::types::SessionGoal;

/// Format the goal for `/goal show` output.
/// Matches Claude Code's format: "Goal active: {condition} ({turns})\nLast check: {reason}"
pub fn format_show(goal: &SessionGoal) -> String {
    match goal.status {
        GoalStatus::Active => {
            let turns = if goal.progress.iterations == 0 {
                "not yet evaluated".to_string()
            } else {
                let n = goal.progress.iterations;
                if n == 1 {
                    "1 turn".to_string()
                } else {
                    format!("{n} turns")
                }
            };
            let last_check = goal
                .progress
                .last_reason
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(|r| format!("\nLast check: {}", r.trim()))
                .unwrap_or_default();
            format!("Goal active: {} ({turns}){last_check}", goal.condition)
        }
        GoalStatus::Paused => format!("Goal paused: {}", goal.condition),
        GoalStatus::Met => "Goal achieved".to_string(),
        GoalStatus::Impossible => "Goal could not be achieved".to_string(),
        GoalStatus::Exhausted => "Goal could not be achieved".to_string(),
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

/// Build the system prompt block for an active goal.
/// Claude Code does not inject a persistent system block — it uses meta messages.
/// Returns `None` always.
pub fn system_prompt_block(_goal: &SessionGoal) -> Option<String> {
    None
}

/// Alias kept for backward compatibility with existing call sites.
pub fn format_system_prompt_block(goal: &SessionGoal) -> Option<String> {
    system_prompt_block(goal)
}
