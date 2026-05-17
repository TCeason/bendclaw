//! Goal command handling.

use super::GoalCoordinator;
use crate::agent::run::runtime::RunFuture;
use crate::agent::session::Session;
use crate::agent::QueryRequest;
use crate::agent::SubmitOutcome;
use crate::error::Result;
use crate::gateway::command::GoalCommand;
use crate::types::GoalBudget;
use crate::types::GoalStatus;
use crate::types::SessionGoal;

pub type StartRun<'a> = Box<dyn Fn(QueryRequest) -> RunFuture<'a> + Send + Sync + 'a>;

pub struct GoalCommandContext<'a> {
    pub goal_verification_enabled: bool,
    pub start_run: StartRun<'a>,
}

pub async fn handle(
    session: &Session,
    request: &QueryRequest,
    sub: GoalCommand,
    ctx: GoalCommandContext<'_>,
) -> Result<SubmitOutcome> {
    match sub {
        GoalCommand::Show => show(session).await,
        GoalCommand::Set {
            condition,
            max_tokens,
            max_iterations,
            max_seconds,
        } => {
            set(
                session,
                request,
                condition,
                GoalBudget {
                    max_tokens,
                    max_iterations,
                    max_seconds,
                },
                ctx,
            )
            .await
        }
        GoalCommand::Pause => match GoalCoordinator::pause(session).await? {
            true => Ok(SubmitOutcome::Command("Goal paused.".into())),
            false => Ok(SubmitOutcome::Command("No goal to pause.".into())),
        },
        GoalCommand::Resume => match GoalCoordinator::resume(session).await? {
            Some(goal) => {
                kickoff_goal_run(session, request, &goal, "Goal resumed.".into(), false, ctx).await
            }
            None => Ok(SubmitOutcome::Command("No goal to resume.".into())),
        },
        GoalCommand::Done { reason } => done(session, reason).await,
        GoalCommand::Clear => {
            let prior = GoalCoordinator::clear(session).await?;
            match prior {
                Some(p) => Ok(SubmitOutcome::Command(format!(
                    "Goal cleared: {}",
                    p.condition
                ))),
                None => Ok(SubmitOutcome::Command("No goal set".into())),
            }
        }
    }
}

async fn show(session: &Session) -> Result<SubmitOutcome> {
    let msg = match session.read_goal().await {
        Some(goal) => crate::agent::goal::display::format_show(&goal),
        None => "No goal set. Usage: `/goal <condition>`".into(),
    };
    Ok(SubmitOutcome::Command(msg))
}

async fn set(
    session: &Session,
    request: &QueryRequest,
    condition: String,
    budget: GoalBudget,
    ctx: GoalCommandContext<'_>,
) -> Result<SubmitOutcome> {
    let condition = crate::agent::goal::validate::validate_condition(&condition)?;
    if !ctx.goal_verification_enabled {
        return Ok(SubmitOutcome::Command(
            "/goal can't run while goal verification is disabled by policy.".into(),
        ));
    }

    GoalCoordinator::set(session, condition, budget).await?;
    let goal = session
        .read_goal()
        .await
        .ok_or_else(|| crate::error::EvotError::Agent("goal not persisted".into()))?;
    kickoff_goal_run(
        session,
        request,
        &goal,
        format!("Goal set: {}", goal.condition),
        true,
        ctx,
    )
    .await
}

async fn done(session: &Session, reason: Option<String>) -> Result<SubmitOutcome> {
    let message = reason.unwrap_or_else(|| "Marked done by user.".into());
    match session.read_goal().await {
        Some(goal) if goal.status == GoalStatus::Met => Ok(SubmitOutcome::Command(
            "Goal is already marked done.".into(),
        )),
        Some(_) => {
            GoalCoordinator::mark_met(session, &message).await?;
            Ok(SubmitOutcome::Command(format!("Goal done: {message}")))
        }
        None => Ok(SubmitOutcome::Command("No goal set".into())),
    }
}

async fn kickoff_goal_run(
    session: &Session,
    request: &QueryRequest,
    goal: &SessionGoal,
    msg: String,
    is_initial_set: bool,
    ctx: GoalCommandContext<'_>,
) -> Result<SubmitOutcome> {
    let prompt = if goal.status == GoalStatus::Active {
        if is_initial_set {
            crate::agent::goal::prompt::goal_set_prompt(&goal.condition)
        } else {
            crate::agent::goal::prompt::continuation_prompt(goal)
        }
    } else {
        return Ok(SubmitOutcome::Command(msg));
    };
    let next = QueryRequest {
        input: vec![evot_engine::Content::Text { text: prompt }],
        session_id: Some(session.session_id().await),
        mode: request.mode.clone(),
        source: request.source.clone(),
    };
    let run = (ctx.start_run)(next).await?;
    Ok(SubmitOutcome::CommandThenRun { msg, run })
}
