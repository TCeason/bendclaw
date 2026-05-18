//! GoalCoordinator — manages goal lifecycle.

use chrono::Utc;

use crate::agent::session::Session;
use crate::error::EvotError;
use crate::error::Result;
use crate::types::GoalBudget;
use crate::types::GoalStatus;
use crate::types::GoalTask;
use crate::types::GoalTaskStatus;
use crate::types::SessionGoal;

pub struct GoalCoordinator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalTaskSummary {
    pub completed: usize,
    pub total: usize,
    pub current: Option<GoalTask>,
    pub tasks: Vec<GoalTask>,
}

impl GoalTaskSummary {
    pub fn headline(&self) -> String {
        let current = self
            .current
            .as_ref()
            .map(|task| format!("current #{} {}", task.id, task.title))
            .unwrap_or_else(|| "no current task".to_string());
        format!(
            "✓ · {}/{} completed · {current}",
            self.completed, self.total
        )
    }
}

impl GoalCoordinator {
    pub async fn set(session: &Session, condition: String, budget: GoalBudget) -> Result<()> {
        let goal = SessionGoal::new(condition, budget);
        session.write_goal(Some(goal)).await?;
        Ok(())
    }

    /// Pause the active goal. Runtime verifier stops continuing while paused.
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

    /// Resume a paused goal, or continue an already active incomplete goal.
    pub async fn resume(session: &Session) -> Result<Option<SessionGoal>> {
        let goal = session.read_goal().await;
        match goal {
            Some(mut g) if g.status == GoalStatus::Paused => {
                g.status = GoalStatus::Active;
                g.touch();
                session.write_goal(Some(g.clone())).await?;
                Ok(Some(g))
            }
            Some(g) if g.status == GoalStatus::Active => Ok(Some(g)),
            _ => Ok(None),
        }
    }

    /// Clear the goal.
    pub async fn clear(session: &Session) -> Result<Option<SessionGoal>> {
        let prior = session.read_goal().await;
        session.write_goal(None).await?;
        Ok(prior)
    }

    /// Store the last verification reason for display.
    pub async fn record_verification_reason(session: &Session, reason: Option<&str>) -> Result<()> {
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

    /// Mark the goal as met. Called by the runtime when the verifier determines
    /// the condition is satisfied.
    pub async fn mark_met(session: &Session, reason: &str) -> Result<()> {
        if let Some(mut g) = session.read_goal().await {
            g.status = GoalStatus::Met;
            g.progress.last_reason = Some(reason.to_string());
            g.touch();
            session.write_goal(Some(g)).await?;
        }
        tracing::info!(reason = %reason, "goal marked as met");
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

    /// Replace the active goal's task list.
    pub async fn update_tasks(session: &Session, tasks: Vec<GoalTask>) -> Result<GoalTaskSummary> {
        validate_tasks(&tasks)?;
        session
            .update_meta(|meta| {
                let goal = meta
                    .goal
                    .as_mut()
                    .ok_or_else(|| EvotError::Session("No active goal to update tasks.".into()))?;
                if goal.status != GoalStatus::Active {
                    return Err(EvotError::Session("No active goal to update tasks.".into()));
                }
                goal.tasks = stamp_task_times(&goal.tasks, tasks);
                goal.touch();
                Ok(Self::task_summary(goal))
            })
            .await
    }

    pub fn task_summary(goal: &SessionGoal) -> GoalTaskSummary {
        GoalTaskSummary {
            completed: goal.completed_task_count(),
            total: goal.tasks.len(),
            current: goal.current_task().cloned(),
            tasks: goal.tasks.clone(),
        }
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

fn stamp_task_times(previous: &[GoalTask], tasks: Vec<GoalTask>) -> Vec<GoalTask> {
    let now = Utc::now().to_rfc3339();
    tasks
        .into_iter()
        .map(|mut task| {
            let previous_task = previous.iter().find(|prior| prior.id == task.id);
            if let Some(prior) = previous_task.filter(|prior| prior.title == task.title) {
                if task.status == prior.status {
                    task.started_at = prior.started_at.clone();
                    task.completed_at = prior.completed_at.clone();
                    return task;
                }

                task.started_at = match task.status {
                    GoalTaskStatus::Pending => None,
                    GoalTaskStatus::InProgress | GoalTaskStatus::Completed => {
                        prior.started_at.clone().or_else(|| Some(now.clone()))
                    }
                };
                task.completed_at = if task.status == GoalTaskStatus::Completed {
                    Some(now.clone())
                } else {
                    None
                };
                return task;
            }

            if task.status == GoalTaskStatus::Pending {
                task.started_at = None;
                task.completed_at = None;
                return task;
            }

            task.started_at = Some(now.clone());
            task.completed_at = if task.status == GoalTaskStatus::Completed {
                Some(now.clone())
            } else {
                None
            };
            task
        })
        .collect()
}

fn validate_tasks(tasks: &[GoalTask]) -> Result<()> {
    use std::collections::HashSet;

    if tasks.is_empty() {
        return Err(EvotError::Session("Goal tasks cannot be empty.".into()));
    }

    let mut ids = HashSet::new();
    let mut in_progress = 0;
    for task in tasks {
        if task.title.trim().is_empty() {
            return Err(EvotError::Session(
                "Goal task title cannot be empty.".into(),
            ));
        }
        if !ids.insert(task.id) {
            return Err(EvotError::Session(format!(
                "Duplicate goal task id: {}.",
                task.id
            )));
        }
        if task.status == GoalTaskStatus::InProgress {
            in_progress += 1;
        }
    }

    if in_progress > 1 {
        return Err(EvotError::Session(
            "Goal tasks can have at most one in_progress task.".into(),
        ));
    }

    Ok(())
}
