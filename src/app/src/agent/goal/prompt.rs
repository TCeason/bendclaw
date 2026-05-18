//! Goal prompt templates.

use crate::types::SessionGoal;

pub fn goal_set_prompt(condition: &str) -> String {
    format!(
        "An active session goal is now set: \"{condition}\". \
         Briefly acknowledge the goal, then immediately start working toward it. \
         First decompose the goal into explicit ordered tasks by calling update_goal_tasks. \
         After planning, execute the first task without waiting for the user. \
         Keep calling update_goal_tasks after meaningful progress. The runtime will keep \
         continuing until the condition holds. It auto-clears once the condition is met — \
         do not tell the user to run `/goal clear` after success; that's only for clearing \
         a goal early."
    )
}

pub fn continuation_prompt(goal: &SessionGoal) -> String {
    let progress = format_progress(goal);
    let task_state = format_task_state(goal);
    format!(
        "Continue working on the goal below. Avoid repeating completed work.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         {progress}\n\n\
         {task_state}",
        condition = goal.condition,
    )
}

fn format_progress(goal: &SessionGoal) -> String {
    let iter_limit = goal
        .budget
        .max_iterations
        .map(|m| m.to_string())
        .unwrap_or_else(|| "∞".into());
    let token_limit = goal
        .budget
        .max_tokens
        .map(|m| m.to_string())
        .unwrap_or_else(|| "∞".into());

    format!(
        "Progress: iteration {iter}/{iter_limit} | {tokens}/{token_limit} tokens",
        iter = goal.progress.iterations,
        tokens = goal.progress.tokens_used,
    )
}

fn format_task_state(goal: &SessionGoal) -> String {
    if goal.tasks.is_empty() {
        return "Tasks: none yet. Call update_goal_tasks with an ordered plan before doing implementation work.".into();
    }

    let current = goal
        .current_task()
        .map(|task| format!("#{} {}", task.id, task.title))
        .unwrap_or_else(|| "none".into());
    let mut lines = vec![format!(
        "Tasks: {}/{} completed. Current: {current}.",
        goal.completed_task_count(),
        goal.tasks.len()
    )];
    lines.push("Work only on the current in_progress task, or the first pending task if none is in_progress. Call update_goal_tasks after meaningful progress. Do not declare the goal complete until all tasks are completed and the user-visible objective is satisfied.".into());
    for task in &goal.tasks {
        let status = match task.status {
            crate::types::GoalTaskStatus::Completed => "completed",
            crate::types::GoalTaskStatus::InProgress => "in_progress",
            crate::types::GoalTaskStatus::Pending => "pending",
        };
        lines.push(format!("- #{} [{}] {}", task.id, status, task.title));
    }
    lines.join("\n")
}
