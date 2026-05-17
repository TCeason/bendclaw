//! Tests for `GoalCoordinator` state transitions and `validate_condition`.

use evot::agent::goal::validate::validate_condition;
use evot::agent::goal::GoalCoordinator;
use evot::agent::session::Session;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use evot::types::GoalBudget;
use evot::types::GoalStatus;
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

    let goal = session.read_goal().await.expect("goal present");
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
    let goal = session.read_goal().await.expect("goal present");
    assert_eq!(goal.status, GoalStatus::Paused);

    let resumed = GoalCoordinator::resume(&session).await?;
    let goal = resumed.expect("resumed goal");
    assert_eq!(goal.status, GoalStatus::Active);
    let goal = session.read_goal().await.expect("goal present");
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
