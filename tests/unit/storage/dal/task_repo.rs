use anyhow::Result;
use bendclaw::storage::TaskRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn task_row(id: &str, lease_token: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![vec![
            serde_json::Value::String(id.to_string()),
            serde_json::Value::String("inst-1".to_string()),
            serde_json::Value::String("nightly-report".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("run report".to_string()),
            serde_json::Value::String("true".to_string()),
            serde_json::Value::String("idle".to_string()),
            serde_json::Value::String("every".to_string()),
            serde_json::Value::String("60".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("false".to_string()),
            serde_json::Value::String("0".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("2026-03-11T00:00:00Z".to_string()),
            serde_json::Value::String(lease_token.to_string()),
            serde_json::Value::String("2026-03-10T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-10T00:00:00Z".to_string()),
        ]],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn task_repo_list_due_scopes_by_executor_instance() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "SELECT id, executor_instance_id, name, cron_expr, prompt, enabled, status, schedule_kind, every_seconds, TO_VARCHAR(at_time), tz, webhook_url, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM tasks WHERE enabled = true AND status != 'running' AND next_run_at <= NOW() AND executor_instance_id = 'inst-1' ORDER BY next_run_at ASC LIMIT 100"
        );
        Ok(task_row("task-1", ""))
    });
    let repo = TaskRepo::new(fake.pool());

    let tasks = repo.list_due("inst-1").await?;

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task-1");
    Ok(())
}

#[tokio::test]
async fn task_repo_claim_due_tasks_updates_then_loads_claimed_rows() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("UPDATE tasks SET status = 'running'") {
            assert!(sql.contains("lease_token = 'lease-1'"));
            assert!(sql.contains("executor_instance_id = 'inst-1'"));
            return Ok(paged_rows(&[], None, None));
        }
        assert_eq!(
            sql,
            "SELECT id, executor_instance_id, name, cron_expr, prompt, enabled, status, schedule_kind, every_seconds, TO_VARCHAR(at_time), tz, webhook_url, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM tasks WHERE lease_token = 'lease-1' AND status = 'running' ORDER BY next_run_at ASC LIMIT 100"
        );
        Ok(task_row("task-1", "lease-1"))
    });
    let repo = TaskRepo::new(fake.pool());

    let tasks = repo.claim_due_tasks("inst-1", "lease-1").await?;

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].lease_token.as_deref(), Some("lease-1"));
    Ok(())
}

#[tokio::test]
async fn task_repo_finish_task_clears_lease_and_updates_status() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(sql.starts_with("UPDATE tasks SET status = 'completed'"));
        assert!(sql.contains("lease_token = NULL"));
        assert!(sql.contains("last_error = NULL"));
        assert!(sql.contains("next_run_at = NULL"));
        assert!(sql.contains("WHERE id = 'task-1' AND lease_token = 'lease-1'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TaskRepo::new(fake.pool());

    repo.finish_task("task-1", "lease-1", "completed", None, None)
        .await?;

    assert_eq!(
        fake.calls(),
        vec![FakeDatabendCall::Query {
            sql: "UPDATE tasks SET status = 'completed', lease_token = NULL, last_run_at = NOW(), run_count = run_count + 1, updated_at = NOW(), last_error = NULL, next_run_at = NULL WHERE id = 'task-1' AND lease_token = 'lease-1'".to_string(),
            database: None,
        }]
    );
    Ok(())
}

#[tokio::test]
async fn task_repo_toggle_and_delete_issue_expected_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(
            sql == "UPDATE tasks SET enabled = NOT enabled, updated_at = NOW() WHERE id = 'task-1'"
                || sql == "DELETE FROM tasks WHERE id = 'task-1'"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = TaskRepo::new(fake.pool());

    repo.toggle("task-1").await?;
    repo.delete("task-1").await?;

    assert_eq!(fake.calls(), vec![
        FakeDatabendCall::Query {
            sql: "UPDATE tasks SET enabled = NOT enabled, updated_at = NOW() WHERE id = 'task-1'"
                .to_string(),
            database: None,
        },
        FakeDatabendCall::Query {
            sql: "DELETE FROM tasks WHERE id = 'task-1'".to_string(),
            database: None,
        },
    ]);
    Ok(())
}
