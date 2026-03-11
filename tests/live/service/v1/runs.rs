use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use axum::body::to_bytes;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::chat;
use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::common::setup::TestContext;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::llm::MockTurn;

#[tokio::test]
async fn create_run_non_stream_returns_run_response() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok from run")))
        .await?;
    let agent_id = uid("run-create-json");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;

    let body = serde_json::json!({
        "session_id": session_id,
        "input": "hello from create_run",
        "stream": false
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = json_body(resp).await?;
    assert_eq!(json["session_id"], session_id.as_str());
    assert_eq!(json["input"], "hello from create_run");
    assert_eq!(json["output"], "ok from run");
    assert_eq!(json["status"], "COMPLETED");
    Ok(())
}

#[tokio::test]
async fn create_run_stream_returns_agno_style_sse() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("streamed reply")))
        .await?;
    let agent_id = uid("run-create-sse");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;

    let body = serde_json::json!({
        "session_id": session_id,
        "input": "stream me",
        "stream": true
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("event: RunStarted"));
    assert!(text.contains("event: RunContent"));
    assert!(text.contains("event: RunCompleted"));
    assert!(text.contains("\"event\":\"RunStarted\""));
    Ok(())
}

// ── list_run_events — event filter ──
//
// Verifies that internal events are filtered out and tool events are visible.
// Uses a tool-call run so both Start/End and ToolStart/ToolEnd are present.

#[tokio::test]
async fn list_run_events_filters_internal_exposes_tool_events() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "shell".into(),
            arguments: r#"{"command":"echo hi"}"#.into(),
        },
        MockTurn::Text("done".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(llm).await?;
    let agent_id = uid("run-ev");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "run echo").await?;
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}/runs"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(resp).await?;
    let runs = body["data"].as_array().context("expected data array")?;
    let run_id = runs[0]["id"]
        .as_str()
        .context("missing run id")?
        .to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/runs/{run_id}/events"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let events: Vec<serde_json::Value> =
        serde_json::from_slice(&axum::body::to_bytes(resp2.into_body(), usize::MAX).await?)?;
    let types: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();

    // Visible events present
    assert!(types.contains(&"Start"), "Start missing: {types:?}");
    assert!(types.contains(&"End"), "End missing: {types:?}");
    assert!(types.contains(&"ToolStart"), "ToolStart missing: {types:?}");
    assert!(types.contains(&"ToolEnd"), "ToolEnd missing: {types:?}");

    // Internal events filtered
    assert!(
        !types.contains(&"TurnStart"),
        "TurnStart should be filtered"
    );
    assert!(!types.contains(&"TurnEnd"), "TurnEnd should be filtered");
    assert!(
        !types.contains(&"ToolUpdate"),
        "ToolUpdate should be filtered"
    );
    assert!(
        !types.contains(&"CheckpointDone"),
        "CheckpointDone should be filtered"
    );

    Ok(())
}
