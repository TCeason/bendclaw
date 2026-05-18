//! Goal display — formatting for `/goal show`.

use crate::types::GoalStatus;
use crate::types::GoalTaskStatus;
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
                "Goal active (not complete yet): {} ({turns}){last_check}\n{}",
                goal.condition,
                format_tasks(goal)
            )
        }
        GoalStatus::Paused => format!(
            "Goal paused (not complete yet): {}\n{}",
            goal.condition,
            format_tasks(goal)
        ),
        GoalStatus::Met => {
            let reason = last_reason
                .map(|r| format!("\nCompletion reason: {r}"))
                .unwrap_or_default();
            format!(
                "Goal complete: {}{reason}\n{}",
                goal.condition,
                format_tasks(goal)
            )
        }
        GoalStatus::Impossible => {
            let reason = last_reason
                .map(|r| format!("\nLast verification: {r}"))
                .unwrap_or_default();
            format!(
                "Goal could not be achieved: {}{reason}\n{}",
                goal.condition,
                format_tasks(goal)
            )
        }
        GoalStatus::Exhausted => format!(
            "Goal exhausted before completion: {}\n{}",
            goal.condition,
            format_tasks(goal)
        ),
    }
}

fn turns_label(iterations: u32) -> String {
    match iterations {
        0 => "not yet verified".to_string(),
        1 => "1 turn".to_string(),
        n => format!("{n} turns"),
    }
}

fn format_tasks(goal: &SessionGoal) -> String {
    if goal.tasks.is_empty() {
        return "Tasks: not planned yet".into();
    }

    let current = goal
        .current_task()
        .map(|task| format!("#{} {}", task.id, task.title))
        .unwrap_or_else(|| "none".into());
    let mut lines = vec![
        format!(
            "Progress: {}/{} completed · current {current}",
            goal.completed_task_count(),
            goal.tasks.len()
        ),
        "".into(),
        "Tasks:".into(),
    ];

    for task in &goal.tasks {
        let marker = match task.status {
            GoalTaskStatus::Completed => "✓",
            GoalTaskStatus::InProgress => "→",
            GoalTaskStatus::Pending => "·",
        };
        lines.push(format!("  {marker} #{} {}", task.id, task.title));
    }

    lines.join("\n")
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
