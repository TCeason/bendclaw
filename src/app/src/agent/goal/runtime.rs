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

    if should_continue_goal_work(&goal) {
        if response_awaits_user(report.transcript) {
            GoalCoordinator::pause(session).await?;
            tracing::info!("goal paused because the assistant is waiting for user input");
            return Ok(AfterTurn::Stop);
        }
        return Ok(AfterTurn::Continue(vec![Content::Text {
            text: prompt::continuation_prompt(&goal),
        }]));
    }

    let Some(verify_fn) = verify_fn else {
        return Ok(AfterTurn::Stop);
    };

    let verdict = super::verify_goal(&goal.condition, report.transcript, |prompt| {
        let verify_fn = verify_fn.clone();
        async move { verify_fn(prompt).await }
    })
    .await?;

    if response_awaits_user(report.transcript) {
        GoalCoordinator::pause(session).await?;
        tracing::info!("goal paused because the assistant is waiting for user input");
        return Ok(AfterTurn::Stop);
    }

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

pub(crate) fn should_continue_goal_work(goal: &crate::types::SessionGoal) -> bool {
    goal.tasks.is_empty() || goal.has_open_tasks()
}

fn response_awaits_user(transcript: &[TranscriptItem]) -> bool {
    let Some(TranscriptItem::Assistant {
        text, tool_calls, ..
    }) = transcript
        .iter()
        .rev()
        .find(|item| matches!(item, TranscriptItem::Assistant { .. }))
    else {
        return false;
    };

    if tool_calls.iter().any(|call| call.name == "ask_user") {
        return true;
    }

    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.ends_with(['?', '？']) {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.contains("should i")
        || lower.contains("do you want")
        || lower.contains("would you like")
        || trimmed.contains("要我")
        || trimmed.contains("是否")
        || trimmed.contains("还是")
        || trimmed.contains("吗？")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_assistant_waiting_for_user_decision() {
        let transcript = vec![TranscriptItem::Assistant {
            text: "I found two viable approaches. Do you want me to continue with the simpler one?"
                .into(),
            thinking: None,
            tool_calls: Vec::new(),
            stop_reason: "stop".into(),
        }];

        assert!(response_awaits_user(&transcript));
    }

    #[test]
    fn ignores_non_question_assistant_progress() {
        let transcript = vec![TranscriptItem::Assistant {
            text: "I will continue with the simpler implementation.".into(),
            thinking: None,
            tool_calls: Vec::new(),
            stop_reason: "stop".into(),
        }];

        assert!(!response_awaits_user(&transcript));
    }
}
