use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::common::setup::TestContext;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn create_and_get_skill() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-cg");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let skill_name = uid("my-skill").to_lowercase();
    let payload = serde_json::json!({
        "name": skill_name,
        "description": "A test skill",
        "content": "echo hello"
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    assert_eq!(created["name"], skill_name.as_str());

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/skills/{skill_name}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let got = json_body(resp2).await?;
    assert_eq!(got["name"], skill_name.as_str());
    assert_eq!(got["content"], "echo hello");
    Ok(())
}

#[tokio::test]
async fn list_skills_returns_full_skill_payload_for_ui() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-full");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let skill_name = uid("full-skill").to_lowercase();
    let payload = serde_json::json!({
        "name": skill_name,
        "description": "A full skill",
        "content": "skill body",
        "executable": true,
        "timeout": 45,
        "parameters": [
            {
                "name": "table",
                "description": "Table name",
                "type": "string",
                "required": true,
                "default": null
            }
        ],
        "files": [
            {
                "path": "scripts/run.sh",
                "body": "#!/usr/bin/env bash\necho hi"
            },
            {
                "path": "references/usage.md",
                "body": "# Usage"
            }
        ],
        "requires": {
            "bins": ["bash"],
            "env": ["API_TOKEN"]
        },
        "manifest": {
            "credentials": [
                {
                    "env": "API_TOKEN",
                    "label": "API Token"
                }
            ]
        }
    });

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(create_resp.status(), StatusCode::OK);

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_resp.status(), StatusCode::OK);
    let body = json_body(list_resp).await?;
    let skill = body
        .as_array()
        .and_then(|skills| skills.iter().find(|s| s["name"] == skill_name))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("skill not found in list response"))?;

    assert_eq!(skill["description"], "A full skill");
    assert_eq!(skill["content"], "skill body");
    assert_eq!(skill["timeout"], 45);
    assert_eq!(skill["created_by_user_id"], user);
    assert_eq!(skill["parameters"][0]["name"], "table");
    assert_eq!(skill["files"][0]["path"], "references/usage.md");
    assert_eq!(skill["files"][1]["path"], "scripts/run.sh");
    assert_eq!(skill["requires"]["env"][0], "API_TOKEN");
    assert_eq!(skill["manifest"]["credentials"][0]["env"], "API_TOKEN");
    Ok(())
}

#[tokio::test]
async fn delete_skill() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-del");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let skill_name = uid("del-skill").to_lowercase();
    let payload = serde_json::json!({
        "name": skill_name,
        "description": "to delete",
        "content": "echo bye"
    });
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/skills/{skill_name}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["deleted"], skill_name.as_str());
    Ok(())
}
