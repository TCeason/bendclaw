//! GoalCoordinator — manages goal lifecycle.

use crate::agent::session::Session;
use crate::error::Result;
use crate::types::GoalBudget;
use crate::types::GoalStatus;
use crate::types::SessionGoal;

pub struct GoalCoordinator;

impl GoalCoordinator {
    pub async fn set(session: &Session, condition: String, budget: GoalBudget) -> Result<()> {
        let goal = SessionGoal::new(condition, budget);
        session.write_goal(Some(goal)).await?;
        Ok(())
    }

    /// Pause the active goal. Runtime policy stops continuing while paused.
    pub async fn pause(session: &Session) -> Result<bool> {
        let goal = session.read_goal().await;
        match goal {
            Some(mut g) if g.status == GoalStatus::Active => {
                g.status = GoalStatus::Paused;
                g.touch();
                session.write_goal(Some(g)).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Resume a paused goal.
    pub async fn resume(session: &Session) -> Result<bool> {
        let goal = session.read_goal().await;
        match goal {
            Some(mut g) if g.status == GoalStatus::Paused => {
                g.status = GoalStatus::Active;
                g.touch();
                session.write_goal(Some(g)).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Clear the goal.
    pub async fn clear(session: &Session) -> Result<Option<SessionGoal>> {
        let prior = session.read_goal().await;
        session.write_goal(None).await?;
        Ok(prior)
    }

    /// Store the last evaluation reason for display.
    pub async fn record_eval_reason(session: &Session, reason: Option<&str>) -> Result<()> {
        let Some(reason) = reason else {
            return Ok(());
        };
        if let Some(mut g) = session.read_goal().await {
            g.progress.last_reason = Some(reason.to_string());
            g.touch();
            session.write_goal(Some(g)).await?;
        }
        Ok(())
    }

    /// Mark the goal as met. Called by the runtime when the evaluator determines
    /// the condition is satisfied.
    pub async fn mark_met(session: &Session, reasoning: &str) -> Result<()> {
        if let Some(mut g) = session.read_goal().await {
            g.status = GoalStatus::Met;
            g.touch();
            session.write_goal(Some(g)).await?;
        }
        tracing::info!(reasoning = %reasoning, "goal marked as met");
        Ok(())
    }

    /// Mark the goal as impossible. Called by the runtime when the evaluator
    /// determines the condition cannot be achieved.
    pub async fn mark_impossible(session: &Session, reasoning: &str) -> Result<()> {
        if let Some(mut g) = session.read_goal().await {
            g.status = GoalStatus::Impossible;
            g.touch();
            session.write_goal(Some(g)).await?;
        }
        tracing::info!(reasoning = %reasoning, "goal marked as impossible");
        Ok(())
    }

    /// Mark the goal as exhausted (budget limit reached).
    pub async fn mark_exhausted(session: &Session) -> Result<()> {
        if let Some(mut g) = session.read_goal().await {
            g.status = GoalStatus::Exhausted;
            g.touch();
            session.write_goal(Some(g)).await?;
        }
        tracing::info!("goal marked as exhausted");
        Ok(())
    }

    /// Account for a completed turn: increment iteration count and accumulate
    /// token/time usage.
    pub async fn account_turn(session: &Session, tokens: u64, elapsed_seconds: u64) -> Result<()> {
        if let Some(mut g) = session.read_goal().await {
            g.progress.iterations += 1;
            g.progress.tokens_used += tokens;
            g.progress.seconds_used += elapsed_seconds;
            g.touch();
            session.write_goal(Some(g)).await?;
        }
        Ok(())
    }
}
