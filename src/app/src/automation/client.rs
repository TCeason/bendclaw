//! Cloud HTTP client for scheduled tasks.

use std::time::Duration;

use serde::de::DeserializeOwned;

use super::model::ClaimedRun;
use super::model::CreatedTask;
use super::model::TaskList;
use crate::auth::AuthState;
use crate::error::EvotError;
use crate::error::Result;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn list_tasks(auth: &AuthState) -> Result<TaskList> {
    request(auth, reqwest::Method::GET, "/v1/tasks", None, None).await
}

pub async fn get_task(auth: &AuthState, task_id: &str) -> Result<serde_json::Value> {
    request(
        auth,
        reqwest::Method::GET,
        &format!("/v1/tasks/{task_id}"),
        None,
        None,
    )
    .await
}

pub async fn create_task(auth: &AuthState, body: &serde_json::Value) -> Result<CreatedTask> {
    request(auth, reqwest::Method::POST, "/v1/tasks", Some(body), None).await
}

pub async fn update_task(
    auth: &AuthState,
    task_id: &str,
    body: &serde_json::Value,
) -> Result<CreatedTask> {
    request(
        auth,
        reqwest::Method::PATCH,
        &format!("/v1/tasks/{task_id}"),
        Some(body),
        None,
    )
    .await
}

pub async fn delete_task(auth: &AuthState, task_id: &str) -> Result<()> {
    request::<serde_json::Value>(
        auth,
        reqwest::Method::DELETE,
        &format!("/v1/tasks/{task_id}"),
        None,
        None,
    )
    .await?;
    Ok(())
}

pub async fn run_task(auth: &AuthState, task_id: &str, request_id: &str) -> Result<()> {
    request::<serde_json::Value>(
        auth,
        reqwest::Method::POST,
        &format!("/v1/tasks/{task_id}/run"),
        Some(&serde_json::json!({})),
        Some(("Idempotency-Key", request_id)),
    )
    .await?;
    Ok(())
}

pub async fn register_executor(
    auth: &AuthState,
    id: &str,
    name: &str,
    channels: serde_json::Value,
) -> Result<()> {
    request::<serde_json::Value>(
        auth,
        reqwest::Method::POST,
        "/v1/task-executors",
        Some(&serde_json::json!({"id": id, "name": name, "channels": channels})),
        None,
    )
    .await?;
    Ok(())
}

/// Claim the next due run for this executor. `Ok(None)` means nothing is due.
pub async fn claim(
    auth: &AuthState,
    executor_id: &str,
    request_id: &str,
) -> Result<Option<ClaimedRun>> {
    let path = "/v1/task-runs/claim";
    let url = format!("{}{}", auth.server_base_url.trim_end_matches('/'), path);
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(&auth.cli_token)
        .json(&serde_json::json!({"executor_id": executor_id, "request_id": request_id}))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| EvotError::Conf(format!("claim task: {error}")))?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    decode(response, path).await.map(Some)
}

pub async fn heartbeat(
    auth: &AuthState,
    run_id: &str,
    lease_token: &str,
    session_id: &str,
) -> Result<()> {
    request::<serde_json::Value>(
        auth,
        reqwest::Method::POST,
        &format!("/v1/task-runs/{run_id}/heartbeat"),
        Some(&serde_json::json!({"session_id": session_id})),
        Some(("X-Evot-Lease", lease_token)),
    )
    .await?;
    Ok(())
}

pub async fn report(
    auth: &AuthState,
    run_id: &str,
    lease_token: &str,
    status: &str,
    summary: &str,
    delivery_status: &str,
    error: &str,
) -> Result<()> {
    request::<serde_json::Value>(
        auth,
        reqwest::Method::POST,
        &format!("/v1/task-runs/{run_id}/report"),
        Some(&serde_json::json!({
            "status": status,
            "result_summary": summary,
            "delivery_status": delivery_status,
            "error": error,
        })),
        Some(("X-Evot-Lease", lease_token)),
    )
    .await?;
    Ok(())
}

async fn request<T: DeserializeOwned>(
    auth: &AuthState,
    method: reqwest::Method,
    path: &str,
    body: Option<&serde_json::Value>,
    header: Option<(&str, &str)>,
) -> Result<T> {
    let url = format!("{}{}", auth.server_base_url.trim_end_matches('/'), path);
    let mut builder = reqwest::Client::new()
        .request(method, url)
        .bearer_auth(&auth.cli_token)
        .timeout(REQUEST_TIMEOUT);
    if let Some(body) = body {
        builder = builder.json(body);
    }
    if let Some((name, value)) = header {
        builder = builder.header(name, value);
    }
    let response = builder
        .send()
        .await
        .map_err(|error| EvotError::Conf(format!("{path}: {error}")))?;
    decode(response, path).await
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response, path: &str) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/error/message")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("server returned {status}"));
        return Err(EvotError::Conf(format!("{path}: {message}")));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| EvotError::Conf(format!("{path}: decode: {error}")))
}
