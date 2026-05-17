//! Goal runtime orchestration.

use std::sync::Arc;
use std::time::Duration;

use evot_engine::Content;

use super::policy;
use super::policy::Decision;
use super::EvalVerdict;
use super::GoalCoordinator;
use crate::agent::run::runtime::EvalFn;
use crate::agent::session::Session;
use crate::error::Result;
use crate::types::GoalStatus;
use crate::types::UsageSummary;

pub enum AfterTurn {
    Continue(Vec<Content>),
    Stop,
}

pub struct GoalTurnReport<'a> {
    pub last_text: &'a str,
    pub tool_summary: &'a str,
    pub usage: &'a UsageSummary,
    pub elapsed: Duration,
}

pub async fn after_turn(
    session: &Arc<Session>,
    eval_fn: Option<&EvalFn>,
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
        report.elapsed.as_secs(),
    )
    .await?;

    let goal = match session.read_goal().await {
        Some(g) => g,
        None => return Ok(AfterTurn::Stop),
    };

    let verdict = evaluate(&goal.condition, eval_fn, &report).await;
    if let Some(ref v) = verdict {
        GoalCoordinator::record_eval_reason(session, v.reason()).await?;
    }

    match policy::decide(&goal, verdict.as_ref()) {
        Decision::Met { .. } => {
            let reasoning = verdict
                .as_ref()
                .and_then(EvalVerdict::reason)
                .unwrap_or_default();
            GoalCoordinator::mark_met(session, reasoning).await?;
            tracing::info!(reasoning = %reasoning, "goal met");
            Ok(AfterTurn::Stop)
        }
        Decision::Impossible { .. } => {
            let reasoning = verdict
                .as_ref()
                .and_then(EvalVerdict::reason)
                .unwrap_or_default();
            GoalCoordinator::mark_impossible(session, reasoning).await?;
            tracing::info!(reasoning = %reasoning, "goal impossible");
            Ok(AfterTurn::Stop)
        }
        Decision::Exhausted { .. } => {
            GoalCoordinator::mark_exhausted(session).await?;
            tracing::info!("goal budget exhausted");
            Ok(AfterTurn::Stop)
        }
        Decision::Continue { prompt } => {
            Ok(AfterTurn::Continue(vec![Content::Text { text: prompt }]))
        }
        Decision::Stop => Ok(AfterTurn::Stop),
    }
}

async fn evaluate(
    condition: &str,
    eval_fn: Option<&EvalFn>,
    report: &GoalTurnReport<'_>,
) -> Option<EvalVerdict> {
    let ef = eval_fn?;
    let summary = transcript_summary(report);
    let ef = ef.clone();
    match super::evaluate_goal(condition, &summary, |prompt| ef(prompt)).await {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(error = %e, "goal eval failed, continuing");
            None
        }
    }
}

fn transcript_summary(report: &GoalTurnReport<'_>) -> String {
    if report.tool_summary.is_empty() {
        report.last_text.to_string()
    } else {
        format!(
            "Tools used: {}\n\nAssistant output:\n{}",
            report.tool_summary, report.last_text
        )
    }
}
