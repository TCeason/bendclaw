//! Tests for goal command handling.

use std::sync::Arc;

use evot::agent::goal::command;
use evot::agent::session::Session;
use evot::agent::QueryRequest;
use evot::agent::Run;
use evot::agent::SubmitOutcome;
use evot::conf::StorageConfig;
use evot::gateway::command::GoalCommand;
use evot::storage::open_storage;
use tempfile::TempDir;
use tokio::sync::mpsc;

async fn fresh_session(
    dir: &TempDir,
) -> std::result::Result<Arc<Session>, Box<dyn std::error::Error>> {
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    Ok(Session::new(
        "sess-goal-command".into(),
        "/tmp".into(),
        "test-model".into(),
        storage,
    )
    .await?)
}

#[tokio::test]
async fn set_persists_goal_and_starts_run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await?;

    let request = QueryRequest {
        input: vec![],
        session_id: Some("sess-goal-command".into()),
        mode: evot::agent::ToolMode::Headless,
        source: "test".into(),
    };

    let outcome = command::handle(
        session.as_ref(),
        &request,
        GoalCommand::Set {
            condition: "write release notes".into(),
            max_tokens: None,
            max_iterations: None,
            max_seconds: None,
        },
        command::GoalCommandContext {
            goal_verification_enabled: true,
            start_run: Box::new(|request| {
                Box::pin(async move {
                    assert!(matches!(
                        request.input.first(),
                        Some(evot_engine::Content::Text { text })
                            if text.contains("write release notes")
                                && text.contains("Do not call any other tool first")
                    ));
                    let (_tx, rx) = mpsc::unbounded_channel();
                    Ok(Run::from_receiver(
                        rx,
                        "sess-goal-command".into(),
                        "run-1".into(),
                    ))
                })
            }),
        },
    )
    .await?;

    assert!(matches!(outcome, SubmitOutcome::CommandThenRun { .. }));
    let Some(goal) = session.read_goal().await else {
        return Err("goal should be persisted".into());
    };
    assert_eq!(goal.condition, "write release notes");
    Ok(())
}
