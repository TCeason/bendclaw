//! Tests for `GoalCoordinator` state transitions and `validate_condition`.

use evot::agent::goal::validate::validate_condition;
use evot::agent::goal::GoalCoordinator;
use evot::agent::session::Session;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use evot::types::GoalBudget;
use evot::types::GoalStatus;
use evot::types::GoalTask;
use evot::types::GoalTaskStatus;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

async fn fresh_session(dir: &TempDir) -> std::sync::Arc<Session> {
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf())).expect("open storage");
    Session::new(
        "sess-goal".into(),
        "/tmp".into(),
        "test-model".into(),
        storage,
    )
    .await
    .expect("session new")
}

#[tokio::test]
async fn set_creates_active_goal_and_persists() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    let budget = GoalBudget {
        max_tokens: Some(50_000),
        max_iterations: None,
        max_seconds: None,
    };
    let condition = validate_condition("  refactor the storage layer  ")?;
    GoalCoordinator::set(&session, condition, budget).await?;

    let goal = session.read_goal().await.ok_or("goal present")?;
    assert_eq!(goal.condition, "refactor the storage layer");
    assert_eq!(goal.status, GoalStatus::Active);
    assert_eq!(goal.budget.max_tokens, Some(50_000));
    assert_eq!(goal.progress.tokens_used, 0);

    Ok(())
}

#[tokio::test]
async fn pause_and_resume_cycle() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    let condition = validate_condition("do work")?;
    GoalCoordinator::set(&session, condition, GoalBudget::default()).await?;

    let paused = GoalCoordinator::pause(&session).await?;
    assert!(paused);
    let goal = session.read_goal().await.ok_or("goal present")?;
    assert_eq!(goal.status, GoalStatus::Paused);

    let resumed = GoalCoordinator::resume(&session).await?;
    let goal = resumed.expect("resumed goal");
    assert_eq!(goal.status, GoalStatus::Active);
    let goal = session.read_goal().await.ok_or("goal present")?;
    assert_eq!(goal.status, GoalStatus::Active);
    Ok(())
}

#[tokio::test]
async fn resume_active_goal_returns_goal_for_continuation() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    let condition = validate_condition("do work")?;
    GoalCoordinator::set(&session, condition, GoalBudget::default()).await?;

    let resumed = GoalCoordinator::resume(&session).await?;
    let goal = resumed.expect("active goal");
    assert_eq!(goal.status, GoalStatus::Active);
    assert_eq!(goal.condition, "do work");
    Ok(())
}

#[tokio::test]
async fn update_tasks_replaces_plan_and_returns_summary() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    GoalCoordinator::set(&session, "ship goal mode".into(), GoalBudget::default()).await?;

    let summary = GoalCoordinator::update_tasks(&session, vec![
        GoalTask::new(1, "Plan work".into(), GoalTaskStatus::Completed),
        GoalTask::new(2, "Implement work".into(), GoalTaskStatus::InProgress),
    ])
    .await?;

    assert_eq!(summary.completed, 1);
    assert_eq!(summary.total, 2);
    assert_eq!(summary.current.as_ref().map(|task| task.id), Some(2));
    let goal = session.read_goal().await.ok_or("goal present")?;
    assert_eq!(goal.tasks.len(), 2);
    assert_eq!(goal.completed_task_count(), 1);
    assert!(goal.tasks[0].started_at.is_some());
    assert!(goal.tasks[0].completed_at.is_some());
    assert!(goal.tasks[1].started_at.is_some());
    assert!(goal.tasks[1].completed_at.is_none());
    assert!(goal.has_open_tasks());
    Ok(())
}

#[tokio::test]
async fn update_tasks_resets_timing_when_title_changes() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    GoalCoordinator::set(&session, "ship goal mode".into(), GoalBudget::default()).await?;

    GoalCoordinator::update_tasks(&session, vec![GoalTask::new(
        1,
        "Original task".into(),
        GoalTaskStatus::Completed,
    )])
    .await?;
    let first_goal = session.read_goal().await.ok_or("goal present")?;
    let first_completed_at = first_goal.tasks[0]
        .completed_at
        .clone()
        .ok_or("completed timestamp")?;

    GoalCoordinator::update_tasks(&session, vec![GoalTask::new(
        1,
        "Different task".into(),
        GoalTaskStatus::Pending,
    )])
    .await?;
    let renamed_goal = session.read_goal().await.ok_or("goal present")?;
    assert_eq!(renamed_goal.tasks[0].title, "Different task");
    assert_eq!(renamed_goal.tasks[0].started_at, None);
    assert_eq!(renamed_goal.tasks[0].completed_at, None);
    assert_ne!(renamed_goal.tasks[0].completed_at, Some(first_completed_at));
    Ok(())
}

#[tokio::test]
async fn update_tasks_preserves_timing_when_task_is_unchanged() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    GoalCoordinator::set(&session, "ship goal mode".into(), GoalBudget::default()).await?;

    GoalCoordinator::update_tasks(&session, vec![GoalTask::new(
        1,
        "Implement work".into(),
        GoalTaskStatus::InProgress,
    )])
    .await?;
    let first_goal = session.read_goal().await.ok_or("goal present")?;
    let first_started_at = first_goal.tasks[0]
        .started_at
        .clone()
        .ok_or("started timestamp")?;

    GoalCoordinator::update_tasks(&session, vec![GoalTask::new(
        1,
        "Implement work".into(),
        GoalTaskStatus::InProgress,
    )])
    .await?;
    let second_goal = session.read_goal().await.ok_or("goal present")?;
    assert_eq!(second_goal.tasks[0].started_at, Some(first_started_at));
    assert_eq!(second_goal.tasks[0].completed_at, None);
    Ok(())
}

#[tokio::test]
async fn update_tasks_rejects_empty_plan() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    GoalCoordinator::set(&session, "ship goal mode".into(), GoalBudget::default()).await?;

    let result = GoalCoordinator::update_tasks(&session, Vec::new()).await;

    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn update_tasks_rejects_non_active_goal() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    GoalCoordinator::set(&session, "ship goal mode".into(), GoalBudget::default()).await?;
    assert!(GoalCoordinator::pause(&session).await?);

    let result = GoalCoordinator::update_tasks(&session, vec![GoalTask::new(
        1,
        "Plan work".into(),
        GoalTaskStatus::InProgress,
    )])
    .await;

    assert!(result.is_err());
    assert!(session
        .read_goal()
        .await
        .ok_or("goal present")?
        .tasks
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn update_tasks_rejects_multiple_in_progress_tasks() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    GoalCoordinator::set(&session, "ship goal mode".into(), GoalBudget::default()).await?;

    let result = GoalCoordinator::update_tasks(&session, vec![
        GoalTask::new(1, "Plan work".into(), GoalTaskStatus::InProgress),
        GoalTask::new(2, "Implement work".into(), GoalTaskStatus::InProgress),
    ])
    .await;

    assert!(result.is_err());
    assert!(session
        .read_goal()
        .await
        .ok_or("goal present")?
        .tasks
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn clear_removes_goal() -> TestResult {
    let dir = TempDir::new()?;
    let session = fresh_session(&dir).await;
    let condition = validate_condition("something")?;
    GoalCoordinator::set(&session, condition, GoalBudget::default()).await?;
    let prior = GoalCoordinator::clear(&session).await?;
    assert_eq!(
        prior.as_ref().map(|g| g.condition.as_str()),
        Some("something")
    );
    assert!(session.read_goal().await.is_none());
    Ok(())
}

#[tokio::test]
async fn validate_condition_rejects_empty_and_oversized_and_control() {
    assert!(validate_condition("   ").is_err());
    let too_big = "x".repeat(5000);
    assert!(validate_condition(&too_big).is_err());
    assert!(validate_condition("hello\x07world").is_err());
    // Forbidden envelope substrings.
    assert!(validate_condition("trying </goal>").is_err());
    assert!(validate_condition("trying <condition>").is_err());
    // Newlines and tabs are fine; double-blank lines collapse.
    let collapsed = validate_condition("a\n\n\nb").expect("ok");
    assert_eq!(collapsed, "a\nb");
}
