//! Cloud session sync: contract of `SessionMeta.cloud`, incremental push with
//! compare-and-append, conflict surfacing, pull into an empty machine, and the
//! cheap state classification used by the `/sessions` list.

use std::sync::Arc;

use evot::auth::AuthState;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use evot::storage::Storage;
use evot::sync;
use evot::types::*;
use serde::Deserialize;
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

const META_V1_WITHOUT_CLOUD: &str = include_str!("fixtures/schema/session-meta-v1-no-cloud.json");
const META_V1_WITH_CLOUD: &str = include_str!("fixtures/schema/session-meta-v1-cloud.json");

/// Shape a reader released before `cloud` existed requires. Strict, so a
/// current writer that renames or drops a published field fails here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct LegacySessionMeta {
    #[serde(default)]
    schema_version: u32,
    session_id: String,
    cwd: String,
    model: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    thinking_level: Option<String>,
    title: Option<String>,
    #[serde(default)]
    custom_title: Option<String>,
    #[serde(default)]
    source: String,
    turns: u32,
    #[serde(default)]
    message_count: u32,
    #[serde(default)]
    context_tokens: usize,
    #[serde(default)]
    context_budget: usize,
    #[serde(default)]
    total_input_tokens: u64,
    #[serde(default)]
    total_output_tokens: u64,
    #[serde(default)]
    span_count: Option<u32>,
    #[serde(default)]
    parent_session_id: Option<String>,
    #[serde(default)]
    fork_seq: Option<u64>,
    created_at: String,
    updated_at: String,
}

fn state(server: &MockServer) -> Result<AuthState, serde_json::Error> {
    serde_json::from_value(json!({
        "version":1, "server_base_url":server.uri(),
        "user":{"id":"user", "name":"User", "email":"user@test.dev"},
        "cli_token":"test-token", "refresh_token":"", "models_synced_at":0
    }))
}

fn user(session: &str, seq: u64, text: &str) -> TranscriptEntry {
    TranscriptEntry::new(session.into(), None, seq, 0, TranscriptItem::User {
        text: text.into(),
        content: vec![],
    })
}

fn fs_storage(root: &TempDir) -> Result<Arc<dyn Storage>, evot::error::EvotError> {
    open_storage(&StorageConfig::fs(root.path().to_path_buf()))
}

#[test]
fn session_meta_cloud_is_backward_and_forward_compatible() -> TestResult {
    let old: SessionMeta = serde_json::from_str(META_V1_WITHOUT_CLOUD)?;
    assert!(old.cloud.is_none());

    let shared: SessionMeta = serde_json::from_str(META_V1_WITH_CLOUD)?;
    let cloud = shared.cloud.as_ref().ok_or("fixture must carry cloud")?;
    assert_eq!(cloud.visibility, CloudVisibility::Public);
    assert_eq!(cloud.synced_seq, 12);
    assert_eq!(
        cloud.public_url.as_deref(),
        Some("https://auto.evot.ai/s/abc")
    );

    // Current writer, legacy strict reader: a local-only session must not
    // even mention `cloud`, so pre-sync readers see exactly what they wrote.
    let written = serde_json::to_string(&old)?;
    let legacy: LegacySessionMeta = serde_json::from_str(&written)?;
    assert_eq!(legacy.session_id, old.session_id);
    assert_eq!(legacy.schema_version, old.schema_version);
    assert!(!written.contains("\"cloud\""));
    Ok(())
}

#[tokio::test]
async fn ordinary_saves_preserve_cloud_state() -> TestResult {
    let root = TempDir::new()?;
    let storage = fs_storage(&root)?;
    let mut meta = SessionMeta::new("s1".into(), "/tmp".into(), "m".into());
    storage.save_session(meta.clone()).await?;
    storage
        .set_session_cloud(
            "s1",
            Some(CloudSync::new(CloudVisibility::Private, "laptop")),
        )
        .await?;

    // A run holding a stale snapshot without `cloud` saves afterwards.
    meta.turns = 3;
    storage.save_session(meta).await?;
    let loaded = storage.get_session("s1").await?.ok_or("missing")?;
    assert_eq!(loaded.turns, 3);
    assert_eq!(
        loaded.cloud.map(|c| c.origin_host),
        Some("laptop".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn share_pushes_full_then_incremental_and_tracks_seq() -> TestResult {
    let server = MockServer::start().await;
    let state = state(&server)?;
    let root = TempDir::new()?;
    let storage = fs_storage(&root)?;
    storage
        .save_session(SessionMeta::new("s1".into(), "/w".into(), "m".into()))
        .await?;
    storage.append_entry(user("s1", 1, "a")).await?;
    storage.append_entry(user("s1", 2, "b")).await?;

    Mock::given(method("PUT"))
        .and(path("/v1/sessions/s1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_partial_json(
            json!({"expected_seq": 0, "visibility": "private"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "seq": 2, "visibility": "private", "updated_at": "t1"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let outcome = sync::share_session(&state, &storage, "s1", None, "test").await?;
    let sync::PushOutcome::Synced { cloud, pushed } = outcome else {
        return Err(format!("expected Synced, got {outcome:?}").into());
    };
    assert_eq!(pushed, 2);
    assert_eq!(cloud.synced_seq, 2);
    // The wire meta never carries per-machine sync state.
    let requests = server.received_requests().await.unwrap_or_default();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body)?;
    assert!(body["meta"].get("cloud").is_none());
    assert_eq!(body["schema_version"], sync::SYNC_SCHEMA_VERSION);

    // Next run appends one entry: only it goes up, at expected_seq 2.
    storage.append_entry(user("s1", 3, "c")).await?;
    Mock::given(method("PUT"))
        .and(path("/v1/sessions/s1"))
        .and(body_partial_json(json!({"expected_seq": 2})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "seq": 3, "visibility": "private"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let outcome = sync::push_session(&state, &storage, "s1", "test", false).await?;
    let sync::PushOutcome::Synced { cloud, pushed } = outcome else {
        return Err(format!("expected Synced, got {outcome:?}").into());
    };
    assert_eq!(pushed, 1);
    assert_eq!(cloud.synced_seq, 3);
    let requests = server.received_requests().await.unwrap_or_default();
    let body: serde_json::Value = serde_json::from_slice(&requests[1].body)?;
    assert_eq!(body["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["entries"][0]["seq"], 3);
    server.verify().await;
    Ok(())
}

#[tokio::test]
async fn push_conflict_is_reported_not_raised_and_leaves_state_untouched() -> TestResult {
    let server = MockServer::start().await;
    let state = state(&server)?;
    let root = TempDir::new()?;
    let storage = fs_storage(&root)?;
    storage
        .save_session(SessionMeta::new("s1".into(), "/w".into(), "m".into()))
        .await?;
    let mut cloud = CloudSync::new(CloudVisibility::Private, "laptop");
    cloud.synced_seq = 2;
    cloud.synced_at = "2020-01-01T00:00:00Z".into();
    storage.set_session_cloud("s1", Some(cloud)).await?;
    for seq in 1..=3 {
        storage.append_entry(user("s1", seq, "x")).await?;
    }
    Mock::given(method("PUT"))
        .and(path("/v1/sessions/s1"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({"seq": 5})))
        .mount(&server)
        .await;
    let outcome = sync::push_session(&state, &storage, "s1", "test", false).await?;
    let sync::PushOutcome::Diverged {
        local_seq,
        remote_seq,
    } = outcome
    else {
        return Err(format!("expected Diverged, got {outcome:?}").into());
    };
    assert_eq!((local_seq, remote_seq), (3, 5));
    let loaded = storage.get_session("s1").await?.ok_or("missing")?;
    assert_eq!(loaded.cloud.map(|c| c.synced_seq), Some(2));
    Ok(())
}

#[tokio::test]
async fn pull_materialises_remote_session_on_a_fresh_machine() -> TestResult {
    let server = MockServer::start().await;
    let state = state(&server)?;
    let root = TempDir::new()?;
    let storage = fs_storage(&root)?;
    let mut remote_meta = SessionMeta::new("s9".into(), "/elsewhere".into(), "m".into());
    remote_meta.title = Some("db migration plan".into());
    remote_meta.turns = 2;
    Mock::given(method("GET"))
        .and(path("/v1/sessions/s9"))
        .and(query_param("after_seq", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema_version": 1,
            "meta": remote_meta,
            "seq": 2,
            "visibility": "private",
            "origin_host": "macbook",
            "entries": [user("s9", 1, "hi"), user("s9", 2, "there")],
        })))
        .expect(1)
        .mount(&server)
        .await;

    let outcome = sync::pull_session(&state, &storage, "s9").await?;
    let sync::PullOutcome::Pulled { meta, appended } = outcome else {
        return Err(format!("expected Pulled, got {outcome:?}").into());
    };
    assert_eq!(appended, 2);
    assert_eq!(meta.title.as_deref(), Some("db migration plan"));
    let cloud = meta.cloud.ok_or("pulled session must be shared")?;
    assert_eq!(cloud.synced_seq, 2);
    assert_eq!(cloud.origin_host, "macbook");
    let entries = storage
        .list_entries(ListTranscriptEntries {
            session_id: "s9".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries.len(), 2);

    // Pulled copy reads as in step: nothing pending on either side.
    let loaded = storage.get_session("s9").await?.ok_or("missing")?;
    let remote = sync::RemoteSession {
        session_id: "s9".into(),
        meta: remote_meta,
        seq: 2,
        visibility: CloudVisibility::Private,
        origin_host: "macbook".into(),
        updated_at: String::new(),
        public_url: None,
    };
    assert_eq!(
        sync::cloud_state(Some(&loaded), Some(&remote)),
        sync::CloudState::Synced
    );
    server.verify().await;
    Ok(())
}

#[tokio::test]
async fn pull_with_local_unpushed_entries_distinguishes_ahead_from_diverged() -> TestResult {
    let server = MockServer::start().await;
    let state = state(&server)?;
    let root = TempDir::new()?;
    let storage = fs_storage(&root)?;
    storage
        .save_session(SessionMeta::new("s1".into(), "/w".into(), "m".into()))
        .await?;
    let mut cloud = CloudSync::new(CloudVisibility::Private, "laptop");
    cloud.synced_seq = 1;
    storage.set_session_cloud("s1", Some(cloud)).await?;
    storage.append_entry(user("s1", 1, "a")).await?;
    storage.append_entry(user("s1", 2, "b")).await?;
    let meta = SessionMeta::new("s1".into(), "/w".into(), "m".into());

    // Server still at 1: local is simply ahead.
    Mock::given(method("GET"))
        .and(path("/v1/sessions/s1"))
        .and(query_param("after_seq", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema_version": 1, "meta": meta, "seq": 1, "visibility": "private", "entries": []
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let outcome = sync::pull_session(&state, &storage, "s1").await?;
    assert!(matches!(outcome, sync::PullOutcome::LocalAhead {
        local_seq: 2,
        remote_seq: 1
    }));

    // Server moved to 4 meanwhile: both sides appended.
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/v1/sessions/s1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schema_version": 1, "meta": meta, "seq": 4, "visibility": "private", "entries": []
        })))
        .mount(&server)
        .await;
    let outcome = sync::pull_session(&state, &storage, "s1").await?;
    assert!(matches!(outcome, sync::PullOutcome::Diverged {
        local_seq: 2,
        remote_seq: 4
    }));
    Ok(())
}

#[test]
fn cloud_state_classifies_from_metadata_alone() {
    let mut local = SessionMeta::new("s".into(), "/w".into(), "m".into());
    let remote = sync::RemoteSession {
        session_id: "s".into(),
        meta: local.clone(),
        seq: 5,
        visibility: CloudVisibility::Private,
        origin_host: String::new(),
        updated_at: String::new(),
        public_url: None,
    };
    assert_eq!(sync::cloud_state(None, None), sync::CloudState::Local);
    assert_eq!(
        sync::cloud_state(None, Some(&remote)),
        sync::CloudState::RemoteOnly
    );
    assert_eq!(
        sync::cloud_state(Some(&local), Some(&remote)),
        sync::CloudState::Local
    );

    local.updated_at = "2024-01-01T00:00:00Z".into();
    let mut cloud = CloudSync::new(CloudVisibility::Private, "h");
    cloud.synced_seq = 5;
    cloud.synced_at = "2024-01-02T00:00:00Z".into();
    local.cloud = Some(cloud.clone());
    assert_eq!(
        sync::cloud_state(Some(&local), Some(&remote)),
        sync::CloudState::Synced
    );
    assert_eq!(
        sync::cloud_state(Some(&local), None),
        sync::CloudState::Synced
    );

    local.updated_at = "2024-01-03T00:00:00Z".into();
    assert_eq!(
        sync::cloud_state(Some(&local), Some(&remote)),
        sync::CloudState::PushPending
    );

    let mut ahead = remote.clone();
    ahead.seq = 7;
    assert_eq!(
        sync::cloud_state(Some(&local), Some(&ahead)),
        sync::CloudState::Diverged
    );
    local.updated_at = "2024-01-01T00:00:00Z".into();
    assert_eq!(
        sync::cloud_state(Some(&local), Some(&ahead)),
        sync::CloudState::PullPending
    );
}
