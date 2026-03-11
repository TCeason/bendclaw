use anyhow::Result;
use bendclaw::storage::TaskHistoryRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

#[tokio::test]
async fn task_history_repo_lists_entries_for_task() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "SELECT id, task_id, run_id, task_name, schedule_kind, cron_expr, prompt, status, output, error, duration_ms, webhook_url, webhook_status, webhook_error, executed_by_instance_id, TO_VARCHAR(created_at) FROM task_history WHERE task_id = 'task-1' ORDER BY created_at DESC LIMIT 10"
        );
        Ok(paged_rows(
            &[&[
                "hist-1",
                "task-1",
                "run-1",
                "nightly-report",
                "every",
                "",
                "run report",
                "ok",
                "done",
                "",
                "1200",
                "",
                "",
                "",
                "inst-1",
                "2026-03-11T00:05:00Z",
            ]],
            None,
            None,
        ))
    });
    let repo = TaskHistoryRepo::new(fake.pool());

    let history = repo.list_by_task("task-1", 10).await?;

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, "hist-1");
    assert_eq!(history[0].task_id, "task-1");
    assert_eq!(history[0].status, "ok");
    assert_eq!(history[0].duration_ms, Some(1200));
    assert_eq!(
        fake.calls(),
        vec![FakeDatabendCall::Query {
            sql: "SELECT id, task_id, run_id, task_name, schedule_kind, cron_expr, prompt, status, output, error, duration_ms, webhook_url, webhook_status, webhook_error, executed_by_instance_id, TO_VARCHAR(created_at) FROM task_history WHERE task_id = 'task-1' ORDER BY created_at DESC LIMIT 10".to_string(),
            database: None,
        }]
    );
    Ok(())
}
