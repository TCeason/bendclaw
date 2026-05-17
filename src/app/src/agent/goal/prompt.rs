//! Goal prompt templates.

use crate::types::SessionGoal;

pub fn goal_set_prompt(condition: &str) -> String {
    format!(
        "An active session goal is now set: \"{condition}\". \
         Briefly acknowledge the goal, then immediately start (or continue) working toward it \
         — treat the condition itself as your directive and do not pause to ask the user \
         what to do. The runtime will keep continuing until the condition holds. It auto-clears once \
         the condition is met — do not tell the user to run `/goal clear` after success; \
         that's only for clearing a goal early."
    )
}

pub fn continuation_prompt(goal: &SessionGoal) -> String {
    let progress = format_progress(goal);
    format!(
        "Continue working on the goal below. Avoid repeating completed work.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         {progress}",
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
