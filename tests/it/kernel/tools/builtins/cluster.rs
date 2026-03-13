use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::client::BendclawClient;
use bendclaw::client::ClusterClient;
use bendclaw::kernel::cluster::ClusterOptions;
use bendclaw::kernel::cluster::ClusterService;
use bendclaw::kernel::tools::builtins::cluster::ClusterCollectTool;
use bendclaw::kernel::tools::builtins::cluster::ClusterDispatchTool;
use bendclaw::kernel::tools::builtins::cluster::ClusterNodesTool;
use bendclaw::kernel::tools::Tool;
use serde_json::Value;

use crate::common::fake_cluster::FakeClusterRegistry;
use crate::common::fake_cluster::FakePeerNode;
use crate::common::fake_cluster::FakeRunPlan;
use crate::common::tracing;
use crate::mocks::context::test_tool_context;

fn make_service(registry_url: &str, auth_token: &str, instance_id: &str) -> Arc<ClusterService> {
    let cluster_client = Arc::new(ClusterClient::new(
        registry_url,
        auth_token,
        instance_id,
        format!("http://{instance_id}.local"),
    ));
    let bendclaw_client = Arc::new(BendclawClient::new(auth_token, Duration::from_secs(5)));
    Arc::new(ClusterService::with_options(
        cluster_client,
        bendclaw_client,
        ClusterOptions {
            heartbeat_interval: Duration::from_millis(100),
            dispatch_poll_interval: Duration::from_millis(25),
        },
    ))
}

#[tokio::test]
async fn cluster_nodes_tool_discovers_registered_peer() -> Result<()> {
    tracing::init();
    let registry = FakeClusterRegistry::start("cluster-test-token").await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        "cluster-test-token",
        "node-peer",
        "http://peer.local",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), "cluster-test-token", "node-self");
    let tool = ClusterNodesTool::new(service);
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    assert!(result.success);

    let nodes: Vec<Value> = serde_json::from_str(&result.output)?;
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["instance_id"], "node-peer");
    assert_eq!(nodes[0]["endpoint"], "http://peer.local");

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_and_collect_tools_complete_remote_run() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-test-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_complete("worker completed")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table.clone());
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let nodes_result = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    assert!(nodes_result.success);

    let dispatch_result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "do work"
            }),
            &ctx,
        )
        .await?;
    assert!(dispatch_result.success);
    let dispatch_json: Value = serde_json::from_str(&dispatch_result.output)?;
    let dispatch_id = dispatch_json["dispatch_id"]
        .as_str()
        .context("dispatch_id missing")?
        .to_string();

    let collect_result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": [dispatch_id],
                "timeout_secs": 2
            }),
            &ctx,
        )
        .await?;
    assert!(collect_result.success);

    let entries: Vec<Value> = serde_json::from_str(&collect_result.output)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "COMPLETED");
    assert_eq!(entries[0]["output"], "worker completed");

    let requests = peer.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].agent_id, "worker-agent");
    assert_eq!(requests[0].input, "do work");
    assert_eq!(requests[0].user_id.as_str(), ctx.user_id.as_ref());
    assert_eq!(
        requests[0].parent_run_id.as_deref(),
        Some(ctx.run_id.as_ref())
    );

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_collect_tool_reports_remote_error() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-test-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_error("remote boom")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table.clone());
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    let dispatch_result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "explode"
            }),
            &ctx,
        )
        .await?;
    let dispatch_json: Value = serde_json::from_str(&dispatch_result.output)?;
    let dispatch_id = dispatch_json["dispatch_id"]
        .as_str()
        .context("dispatch_id missing")?
        .to_string();

    let collect_result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": [dispatch_id],
                "timeout_secs": 2
            }),
            &ctx,
        )
        .await?;
    assert!(collect_result.success);

    let entries: Vec<Value> = serde_json::from_str(&collect_result.output)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "ERROR");
    assert!(entries[0]["error"]
        .as_str()
        .is_some_and(|error| error.contains("remote boom")));

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}
