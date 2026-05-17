//! Goal runtime orchestration.

use std::sync::Arc;

use evot_engine::Content;

use super::prompt;
use super::GoalCoordinator;
use super::GoalVerdict;
use crate::agent::run::runtime::VerifyFn;
use crate::agent::session::Session;
use crate::error::Result;
use crate::types::GoalStatus;
use crate::types::TranscriptItem;
use crate::types::UsageSummary;

pub enum AfterTurn {
    Continue(Vec<Content>),
    Stop,
}

pub struct GoalTurnReport<'a> {
    pub transcript: &'a [TranscriptItem],
    pub usage: &'a UsageSummary,
    pub elapsed_seconds: u64,
}

pub async fn after_turn(
    session: &Arc<Session>,
    verify_fn: Option<&VerifyFn>,
    report: GoalTurnReport<'_>,
) -> Result<AfterTurn> {
    if !matches!(
        session.read_goal().await,
        Some(g) if g.status == GoalStatus::Active
    ) {
        return Ok(AfterTurn::Stop);
    }

    GoalCoordinator::account_turn(
        session,
        report.usage.input.saturating_add(report.usage.output),
        report.elapsed_seconds,
    )
    .await?;

    let goal = match session.read_goal().await {
        Some(g) if g.status == GoalStatus::Active => g,
        _ => return Ok(AfterTurn::Stop),
    };

    if goal.is_budget_exhausted() {
        GoalCoordinator::mark_exhausted(session).await?;
        tracing::info!("goal budget exhausted");
        return Ok(AfterTurn::Stop);
    }

    let Some(verify_fn) = verify_fn else {
        return Ok(AfterTurn::Stop);
    };

    let verdict = super::verify_goal(&goal.condition, report.transcript, |prompt| {
        let verify_fn = verify_fn.clone();
        async move { verify_fn(prompt).await }
    })
    .await?;

    GoalCoordinator::record_verification_reason(session, Some(verdict.reason())).await?;

    match verdict {
        GoalVerdict::Met { reason } => {
            GoalCoordinator::mark_met(session, &reason).await?;
            tracing::info!(reason = %reason, "goal met");
            Ok(AfterTurn::Stop)
        }
        GoalVerdict::NotMet { .. } => Ok(AfterTurn::Continue(vec![Content::Text {
            text: prompt::continuation_prompt(&goal),
        }])),
    }
}
