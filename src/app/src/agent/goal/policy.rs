//! Goal policy — decides whether to continue, stop, or wrap up.
//!
//! Pure function: takes the current goal state + optional eval verdict and
//! returns a Decision. No side effects.

use super::prompt;
use super::EvalVerdict;
use crate::types::GoalStatus;
use crate::types::SessionGoal;

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Decision {
    /// Continue working: inject the continuation prompt as the next user message.
    Continue { prompt: String },
    /// Goal condition met: inject wrap-up prompt, then stop.
    Met { wrap_prompt: String },
    /// Goal condition impossible: inject wrap-up prompt, then stop.
    Impossible { wrap_prompt: String },
    /// Budget exhausted: inject wrap-up prompt for a final summary turn.
    Exhausted { wrap_prompt: String },
    /// Stop immediately (paused or already terminal).
    Stop,
}

// ---------------------------------------------------------------------------
// decide
// ---------------------------------------------------------------------------

/// Determine the next action for the goal loop.
///
/// Called after each turn with the current goal state and the optional eval verdict.
pub fn decide(goal: &SessionGoal, verdict: Option<&EvalVerdict>) -> Decision {
    // Terminal or paused goals always stop.
    match goal.status {
        GoalStatus::Paused | GoalStatus::Met | GoalStatus::Impossible | GoalStatus::Exhausted => {
            return Decision::Stop;
        }
        GoalStatus::Active => {}
    }

    // Budget check takes priority over eval verdict.
    if goal.is_budget_exhausted() {
        return Decision::Exhausted {
            wrap_prompt: prompt::exhausted_prompt(goal),
        };
    }

    // If we have an eval verdict, act on it.
    if let Some(v) = verdict {
        match v {
            EvalVerdict::Met { reasoning } => {
                return Decision::Met {
                    wrap_prompt: prompt::met_prompt(goal, reasoning),
                };
            }
            EvalVerdict::Impossible { reasoning } => {
                return Decision::Impossible {
                    wrap_prompt: prompt::impossible_prompt(goal, reasoning),
                };
            }
            EvalVerdict::Continue => {}
        }
    }

    // Default: continue working.
    Decision::Continue {
        prompt: prompt::continuation_prompt(goal),
    }
}
