//! Goal prompt templates — continuation, wrap-up, and evaluation prompts.

use crate::types::SessionGoal;

// ---------------------------------------------------------------------------
// Goal set meta message — injected as the continuation prompt when goal is set
// ---------------------------------------------------------------------------

pub fn goal_set_prompt(condition: &str) -> String {
    format!(
        "An active session goal is now set: \"{condition}\". \
         Briefly acknowledge the goal, then immediately start (or continue) working toward it \
         \u{2014} treat the condition itself as your directive and do not pause to ask the user \
         what to do. The runtime will keep continuing until the condition holds. It auto-clears once \
         the condition is met \u{2014} do not tell the user to run `/goal clear` after success; \
         that's only for clearing a goal early."
    )
}

// ---------------------------------------------------------------------------
// Continuation prompt — injected as user message to keep the model working
// ---------------------------------------------------------------------------

pub fn continuation_prompt(goal: &SessionGoal) -> String {
    let progress = format_progress(goal);
    format!(
        "Continue working on the goal below. Avoid repeating completed work.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         {progress}",
        condition = goal.condition,
    )
}

// ---------------------------------------------------------------------------
// Terminal prompts — injected for a final summary turn before stopping
// ---------------------------------------------------------------------------

pub fn met_prompt(goal: &SessionGoal, reasoning: &str) -> String {
    format!(
        "The goal condition has been evaluated as **met**.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         Evaluator reasoning: {reasoning}\n\n\
         Provide a brief summary of what was accomplished.",
        condition = goal.condition,
    )
}

pub fn impossible_prompt(goal: &SessionGoal, reasoning: &str) -> String {
    format!(
        "The goal condition has been evaluated as **impossible** to achieve.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         Evaluator reasoning: {reasoning}\n\n\
         Explain what was attempted and why the goal cannot be completed.",
        condition = goal.condition,
    )
}

pub fn exhausted_prompt(goal: &SessionGoal) -> String {
    let progress = format_progress(goal);
    format!(
        "Budget exhausted for the current goal. Wrap up your work.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         {progress}\n\n\
         Summarize what was accomplished and what remains.",
        condition = goal.condition,
    )
}

// ---------------------------------------------------------------------------
// System prompt block — no longer injected; Claude Code uses meta messages only
// ---------------------------------------------------------------------------

pub fn system_block(_goal: &SessionGoal) -> String {
    String::new()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
