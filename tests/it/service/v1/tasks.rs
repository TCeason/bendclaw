use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[derive(Clone)]
struct TaskState {
    records: Arc<Mutex<Vec<TaskRecord>>>,
    history: Arc<Mutex<Vec<TaskHistoryRecord>>>,
}

#[derive(Clone)]
struct TaskRecord {
    id: String,
    executor_instance_id: String,
    name: String,
    prompt: String,
    enabled: bool,
    status: String,
    schedule_kind: String,
    every_seconds: Option<i32>,
    next_run_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Clone)]
struct TaskHistoryRecord {
    id: String,
    task_id: String,
    task_name: String,
    status: String,
    output: Option<String>,
    duration_ms: Option<i32>,
    created_at: String,
}

fn quoted_values(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(next) = chars.next() {
            if next == '\'' {
                if chars.peek() == Some(&'\'') {
                    value.push('\'');
                    chars.next();
                    continue;
                }
                break;
            }
            value.push(next);
        }
        out.push(value);
    }
    out
}

fn task_rows(records: &[TaskRecord]) -> bendclaw::storage::pool::QueryResponse {
    let data = records
        .iter()
        .map(|record| {
            vec![
                serde_json::Value::String(record.id.clone()),
                serde_json::Value::String(record.executor_instance_id.clone()),
                serde_json::Value::String(record.name.clone()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(record.prompt.clone()),
                serde_json::Value::String(record.enabled.to_string()),
                serde_json::Value::String(record.status.clone()),
                serde_json::Value::String(record.schedule_kind.clone()),
                serde_json::Value::String(
                    record
                        .every_seconds
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String("false".to_string()),
                serde_json::Value::String("0".to_string()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(record.next_run_at.clone().unwrap_or_default()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(record.created_at.clone()),
                serde_json::Value::String(record.updated_at.clone()),
            ]
        })
        .collect();
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data,
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn task_history_rows(records: &[TaskHistoryRecord]) -> bendclaw::storage::pool::QueryResponse {
    let data = records
        .iter()
        .map(|record| {
            vec![
                serde_json::Value::String(record.id.clone()),
                serde_json::Value::String(record.task_id.clone()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(record.task_name.clone()),
                serde_json::Value::String("every".to_string()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String("run report".to_string()),
                serde_json::Value::String(record.status.clone()),
                serde_json::Value::String(record.output.clone().unwrap_or_default()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(
                    record
                        .duration_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(String::new()),
                serde_json::Value::String(record.created_at.clone()),
            ]
        })
        .collect();
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data,
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn tasks_api_fast_create_list_and_toggle() -> Result<()> {
    let state = TaskState {
        records: Arc::new(Mutex::new(Vec::new())),
        history: Arc::new(Mutex::new(Vec::new())),
    };
    let fake_state = state.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        let mut records = fake_state.records.lock().expect("task state");
        if sql.starts_with("INSERT INTO tasks") {
            let values = quoted_values(sql);
            records.push(TaskRecord {
                id: values[0].clone(),
                executor_instance_id: values[1].clone(),
                name: values[2].clone(),
                prompt: values[4].clone(),
                enabled: true,
                status: values[5].clone(),
                schedule_kind: values[6].clone(),
                every_seconds: Some(60),
                next_run_at: Some("2026-03-11T00:00:00Z".to_string()),
                created_at: "2026-03-10T00:00:00Z".to_string(),
                updated_at: "2026-03-10T00:00:00Z".to_string(),
            });
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT COUNT(*) FROM tasks") {
            let count = records.len().to_string();
            return Ok(paged_rows(&[&[count.as_str()]], None, None));
        }
        if sql.starts_with("SELECT id, executor_instance_id") && sql.contains("WHERE id = ") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            let found: Vec<_> = records
                .iter()
                .filter(|record| record.id == id)
                .cloned()
                .collect();
            return Ok(task_rows(&found));
        }
        if sql.starts_with("SELECT id, executor_instance_id") {
            let mut all = records.clone();
            all.reverse();
            return Ok(task_rows(&all));
        }
        if sql.starts_with("UPDATE tasks SET enabled = NOT enabled") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            if let Some(record) = records.iter_mut().find(|record| record.id == id) {
                record.enabled = !record.enabled;
                record.updated_at = "2026-03-11T00:10:00Z".to_string();
            }
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_task_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": "nightly-report",
                    "prompt": "run report",
                    "schedule": {
                        "kind": "every",
                        "seconds": 60
                    }
                }))?))?,
        )
        .await?;
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await?;
    let task_id = created_body["id"].as_str().expect("task id").to_string();
    assert_eq!(created_body["name"], "nightly-report");
    assert_eq!(created_body["schedule_kind"], "every");

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body["data"][0]["id"], task_id);
    assert_eq!(list_body["data"][0]["enabled"], true);

    let toggled = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/tasks/{task_id}/toggle"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(toggled.status(), StatusCode::OK);
    let toggled_body = json_body(toggled).await?;
    assert_eq!(toggled_body["enabled"], false);
    Ok(())
}

#[tokio::test]
async fn tasks_api_fast_update_delete_and_history() -> Result<()> {
    let state = TaskState {
        records: Arc::new(Mutex::new(vec![TaskRecord {
            id: "task-1".to_string(),
            executor_instance_id: "test_instance".to_string(),
            name: "nightly-report".to_string(),
            prompt: "run report".to_string(),
            enabled: true,
            status: "idle".to_string(),
            schedule_kind: "every".to_string(),
            every_seconds: Some(60),
            next_run_at: Some("2026-03-11T00:00:00Z".to_string()),
            created_at: "2026-03-10T00:00:00Z".to_string(),
            updated_at: "2026-03-10T00:00:00Z".to_string(),
        }])),
        history: Arc::new(Mutex::new(vec![TaskHistoryRecord {
            id: "hist-1".to_string(),
            task_id: "task-1".to_string(),
            task_name: "nightly-report".to_string(),
            status: "ok".to_string(),
            output: Some("done".to_string()),
            duration_ms: Some(1200),
            created_at: "2026-03-11T00:05:00Z".to_string(),
        }])),
    };
    let fake_state = state.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("SELECT COUNT(*) FROM task_history") {
            let count = fake_state
                .history
                .lock()
                .expect("task history")
                .len()
                .to_string();
            return Ok(paged_rows(&[&[count.as_str()]], None, None));
        }
        if sql.starts_with("SELECT COUNT(*) FROM tasks") {
            let count = fake_state
                .records
                .lock()
                .expect("task state")
                .len()
                .to_string();
            return Ok(paged_rows(&[&[count.as_str()]], None, None));
        }
        if sql.starts_with("SELECT id, task_id, run_id") {
            let history = fake_state.history.lock().expect("task history").clone();
            return Ok(task_history_rows(&history));
        }

        let mut records = fake_state.records.lock().expect("task state");
        if sql.starts_with("SELECT id, executor_instance_id") && sql.contains("WHERE id = ") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            let found: Vec<_> = records
                .iter()
                .filter(|record| record.id == id)
                .cloned()
                .collect();
            return Ok(task_rows(&found));
        }
        if sql.starts_with("UPDATE tasks SET ") {
            assert!(sql.contains("name = 'updated-report'"));
            assert!(sql.contains("enabled = false"));
            let id = quoted_values(sql).last().cloned().unwrap_or_default();
            if let Some(record) = records.iter_mut().find(|record| record.id == id) {
                record.name = "updated-report".to_string();
                record.enabled = false;
                record.updated_at = "2026-03-11T00:10:00Z".to_string();
            }
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("DELETE FROM tasks WHERE id = ") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            records.retain(|record| record.id != id);
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_task_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let updated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/tasks/task-1"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": "updated-report",
                    "enabled": false
                }))?))?,
        )
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body = json_body(updated).await?;
    assert_eq!(updated_body["name"], "updated-report");
    assert_eq!(updated_body["enabled"], false);

    let history = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/tasks/task-1/history"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(history.status(), StatusCode::OK);
    let history_body = json_body(history).await?;
    assert_eq!(history_body["data"][0]["id"], "hist-1");
    assert_eq!(history_body["data"][0]["status"], "ok");
    assert_eq!(history_body["data"][0]["duration_ms"], 1200);

    let deleted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/tasks/task-1"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    let deleted_body = json_body(deleted).await?;
    assert_eq!(deleted_body["deleted"], "task-1");

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body["data"].as_array().map(Vec::len), Some(0));
    Ok(())
}
