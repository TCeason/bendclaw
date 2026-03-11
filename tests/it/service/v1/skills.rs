use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn create_skill_overwrites_same_name_within_agent_fast() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.contains("FROM agent_config") || sql.contains("SHOW DATABASES LIKE") {
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_skill_{}_",
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
    let skill_name = "report-skill";
    setup_agent(&app, &agent_id, &user).await?;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": skill_name,
                    "description": "first version",
                    "content": "first body",
                    "files": [{
                        "path": "references/old.md",
                        "body": "# old"
                    }]
                }))?))?,
        )
        .await?;
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": skill_name,
                    "description": "second version",
                    "content": "second body",
                    "files": [{
                        "path": "references/new.md",
                        "body": "# new"
                    }]
                }))?))?,
        )
        .await?;
    assert_eq!(second.status(), StatusCode::OK);

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/skills/{skill_name}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let skill = json_body(get_resp).await?;
    assert_eq!(skill["description"], "second version");
    assert_eq!(skill["content"], "second body");
    assert_eq!(skill["created_by_user_id"], user);
    assert_eq!(skill["files"][0]["path"], "references/new.md");

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
    let skills = json_body(list_resp).await?;
    let items = skills.as_array().expect("skill list should be an array");
    let matches: Vec<_> = items
        .iter()
        .filter(|skill| skill["name"] == skill_name)
        .collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["description"], "second version");
    assert_eq!(matches[0]["files"][0]["path"], "references/new.md");
    Ok(())
}
