use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use bendclaw::kernel::tools::task::TaskCreateTool;
use bendclaw::kernel::tools::task::TaskDeleteTool;
use bendclaw::kernel::tools::task::TaskGetTool;
use bendclaw::kernel::tools::task::TaskHistoryTool;
use bendclaw::kernel::tools::task::TaskListTool;
use bendclaw::kernel::tools::task::TaskToggleTool;
use bendclaw::kernel::tools::task::TaskUpdateTool;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::mocks::context::test_workspace;

fn ctx_with_pool(pool: bendclaw::storage::Pool) -> bendclaw::kernel::tools::ToolContext {
    bendclaw::kernel::tools::ToolContext {
        user_id: "u1".into(),
        session_id: "s1".into(),
        agent_id: "a1".into(),
        workspace: test_workspace(
            std::env::temp_dir().join(format!("bendclaw-task-tool-{}", ulid::Ulid::new())),
        ),
        pool,
    }
}

fn task_row_vec(id: &str, name: &str, enabled: bool) -> Vec<String> {
    vec![
        id.to_string(),
        "inst-1".to_string(),
        name.to_string(),
        "".to_string(),
        "run report".to_string(),
        enabled.to_string(),
        "idle".to_string(),
        "every".to_string(),
        "60".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "".to_string(),
        "false".to_string(),
        "0".to_string(),
        "".to_string(),
        "2026-03-11T00:00:00Z".to_string(),
        "".to_string(),
        "2026-03-10T00:00:00Z".to_string(),
        "2026-03-10T00:00:00Z".to_string(),
    ]
}

fn row_response(row: &[String]) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![row
            .iter()
            .map(|value| serde_json::Value::String(value.clone()))
            .collect()],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn history_row_response() -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![vec![
            serde_json::Value::String("hist-1".to_string()),
            serde_json::Value::String("task-1".to_string()),
            serde_json::Value::String("run-1".to_string()),
            serde_json::Value::String("nightly-report".to_string()),
            serde_json::Value::String("every".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("run report".to_string()),
            serde_json::Value::String("ok".to_string()),
            serde_json::Value::String("done".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("1200".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("inst-1".to_string()),
            serde_json::Value::String("2026-03-11T00:05:00Z".to_string()),
        ]],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn task_create_tool_persists_schedule_and_returns_json() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(sql.contains("INSERT INTO tasks"));
        assert!(sql.contains("'report-task'"));
        assert!(sql.contains("'every'"));
        assert!(sql.contains("'inst-1'"));
        Ok(paged_rows(&[], None, None))
    });
    let tool = TaskCreateTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(
            json!({
                "name": "report-task",
                "prompt": "run report",
                "schedule": {
                    "kind": "every",
                    "seconds": 60
                }
            }),
            &ctx_with_pool(fake.pool()),
        )
        .await?;

    assert!(result.success);
    let body: serde_json::Value = serde_json::from_str(&result.output)?;
    assert_eq!(body["name"], "report-task");
    assert_eq!(body["schedule_kind"], "every");
    assert_eq!(body["enabled"], true);
    Ok(())
}

#[tokio::test]
async fn task_list_tool_returns_compact_items() -> Result<()> {
    let row = task_row_vec("task-1", "nightly-report", true);
    let fake = FakeDatabend::new(move |sql, _database| {
        assert!(sql.contains("FROM tasks"));
        assert!(sql.contains("ORDER BY created_at DESC"));
        assert!(sql.contains("LIMIT 2"));
        Ok(row_response(&row))
    });
    let tool = TaskListTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(json!({"limit": 2}), &ctx_with_pool(fake.pool()))
        .await?;

    assert!(result.success);
    let body: serde_json::Value = serde_json::from_str(&result.output)?;
    assert_eq!(body[0]["id"], "task-1");
    assert_eq!(body[0]["name"], "nightly-report");
    assert_eq!(body[0]["schedule_kind"], "every");
    Ok(())
}

#[tokio::test]
async fn task_get_tool_returns_full_record() -> Result<()> {
    let row = task_row_vec("task-1", "nightly-report", true);
    let fake = FakeDatabend::new(move |sql, _database| {
        assert!(sql.contains("FROM tasks"));
        assert!(sql.contains("WHERE id = 'task-1'"));
        assert!(sql.contains("LIMIT 1"));
        Ok(row_response(&row))
    });
    let tool = TaskGetTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(json!({"task_id": "task-1"}), &ctx_with_pool(fake.pool()))
        .await?;

    assert!(result.success);
    let body: serde_json::Value = serde_json::from_str(&result.output)?;
    assert_eq!(body["id"], "task-1");
    assert_eq!(body["name"], "nightly-report");
    assert_eq!(body["every_seconds"], 60);
    Ok(())
}

#[tokio::test]
async fn task_update_tool_reads_updates_and_reloads_task() -> Result<()> {
    let calls = Arc::new(Mutex::new(0usize));
    let row = task_row_vec("task-1", "updated-report", false);
    let calls_for_fake = Arc::clone(&calls);
    let fake = FakeDatabend::new(move |sql, _database| {
        let mut call = calls_for_fake.lock().expect("task update call count");
        *call += 1;
        match *call {
            1 => {
                assert!(sql.contains("WHERE id = 'task-1'"));
                Ok(paged_rows(
                    &[&[
                        "task-1",
                        "inst-1",
                        "old-report",
                        "",
                        "run report",
                        "true",
                        "idle",
                        "every",
                        "60",
                        "",
                        "",
                        "",
                        "",
                        "false",
                        "0",
                        "",
                        "2026-03-11T00:00:00Z",
                        "",
                        "2026-03-10T00:00:00Z",
                        "2026-03-10T00:00:00Z",
                    ]],
                    None,
                    None,
                ))
            }
            2 => {
                assert!(sql.contains("UPDATE tasks SET"));
                assert!(sql.contains("name = 'updated-report'"));
                assert!(sql.contains("enabled = false"));
                Ok(paged_rows(&[], None, None))
            }
            3 => {
                assert!(sql.contains("WHERE id = 'task-1'"));
                Ok(row_response(&row))
            }
            other => panic!("unexpected query count {other}: {sql}"),
        }
    });
    let tool = TaskUpdateTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(
            json!({
                "task_id": "task-1",
                "name": "updated-report",
                "enabled": false
            }),
            &ctx_with_pool(fake.pool()),
        )
        .await?;

    assert!(result.success);
    let body: serde_json::Value = serde_json::from_str(&result.output)?;
    assert_eq!(body["name"], "updated-report");
    assert_eq!(body["enabled"], false);
    Ok(())
}

#[tokio::test]
async fn task_delete_tool_removes_task() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(sql, "DELETE FROM tasks WHERE id = 'task-1'");
        Ok(paged_rows(&[], None, None))
    });
    let tool = TaskDeleteTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(json!({"task_id": "task-1"}), &ctx_with_pool(fake.pool()))
        .await?;

    assert!(result.success);
    assert_eq!(result.output, "Task 'task-1' deleted");
    Ok(())
}

#[tokio::test]
async fn task_toggle_tool_returns_updated_task_summary() -> Result<()> {
    let calls = Arc::new(Mutex::new(0usize));
    let row = task_row_vec("task-1", "nightly-report", false);
    let calls_for_fake = Arc::clone(&calls);
    let fake = FakeDatabend::new(move |sql, _database| {
        let mut call = calls_for_fake.lock().expect("task toggle call count");
        *call += 1;
        match *call {
            1 => {
                assert_eq!(
                    sql,
                    "UPDATE tasks SET enabled = NOT enabled, updated_at = NOW() WHERE id = 'task-1'"
                );
                Ok(paged_rows(&[], None, None))
            }
            2 => {
                assert!(sql.contains("WHERE id = 'task-1'"));
                Ok(row_response(&row))
            }
            other => panic!("unexpected query count {other}: {sql}"),
        }
    });
    let tool = TaskToggleTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(json!({"task_id": "task-1"}), &ctx_with_pool(fake.pool()))
        .await?;

    assert!(result.success);
    let body: serde_json::Value = serde_json::from_str(&result.output)?;
    assert_eq!(body["id"], "task-1");
    assert_eq!(body["enabled"], false);
    assert_eq!(body["status"], "idle");
    Ok(())
}

#[tokio::test]
async fn task_history_tool_returns_entries() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "SELECT id, task_id, run_id, task_name, schedule_kind, cron_expr, prompt, status, output, error, duration_ms, webhook_url, webhook_status, webhook_error, executed_by_instance_id, TO_VARCHAR(created_at) FROM task_history WHERE task_id = 'task-1' ORDER BY created_at DESC LIMIT 5"
        );
        Ok(history_row_response())
    });
    let tool = TaskHistoryTool::new("inst-1".to_string());

    let result = tool
        .execute_with_context(
            json!({"task_id": "task-1", "limit": 5}),
            &ctx_with_pool(fake.pool()),
        )
        .await?;

    assert!(result.success);
    let body: serde_json::Value = serde_json::from_str(&result.output)?;
    assert_eq!(body[0]["id"], "hist-1");
    assert_eq!(body[0]["status"], "ok");
    assert_eq!(body[0]["duration_ms"], 1200);
    Ok(())
}
