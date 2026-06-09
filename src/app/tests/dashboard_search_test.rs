//! Tests for the dashboard session-search feature.
//!
//! The web search UI (dashboard shell + /chat page) does client-side substring
//! filtering and highlighting over the `search_text` field returned by
//! `list_sessions_with_text`. These tests pin the backend contract that feeds
//! that UI, plus a guard that the dashboard shell actually embeds the search
//! markup that calls `/api/sessions`.

use std::sync::Arc;

use evot::agent::session::Session;
use evot::agent::*;
use evot::storage::MemoryStorage;
use evot::types::ListSessions;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn assistant(text: &str) -> TranscriptItem {
    TranscriptItem::Assistant {
        text: text.into(),
        thinking: None,
        tool_calls: vec![],
        stop_reason: "stop".into(),
        usage: UsageSummary::default(),
        model: String::new(),
        provider: String::new(),
        timestamp: 0,
        error_message: None,
    }
}

#[tokio::test]
async fn search_text_includes_transcript_content() -> TestResult {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());

    let session = Session::new(
        "search-sess-001".into(),
        "/home/me/project".into(),
        "test-model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "how do I configure the retry budget".into(),
                content: vec![],
            },
            assistant("Set max_retries in the provider config block."),
        ])
        .await?;

    let rows = storage.list_sessions_with_text(10).await?;
    let row = rows
        .iter()
        .find(|r| r.session.session_id == "search-sess-001")
        .ok_or("session not returned")?;

    // The flattened text the UI filters and highlights against must carry
    // metadata (cwd) and transcript content from both roles.
    assert!(row.search_text.contains("/home/me/project"));
    assert!(row.search_text.contains("retry budget"));
    assert!(row.search_text.contains("max_retries"));
    Ok(())
}

#[tokio::test]
async fn list_sessions_with_text_respects_limit() -> TestResult {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());

    for i in 0..5 {
        let s = Session::new(
            format!("limit-sess-{i:03}"),
            "/tmp".into(),
            "test-model".into(),
            storage.clone(),
        )
        .await?;
        s.write_items(vec![assistant("hello")]).await?;
    }

    let total = storage.list_sessions(ListSessions { limit: 100 }).await?;
    assert_eq!(total.len(), 5);

    let limited = storage.list_sessions_with_text(2).await?;
    assert_eq!(limited.len(), 2);
    Ok(())
}

#[tokio::test]
async fn delete_session_removes_only_target() -> TestResult {
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());

    for id in ["del-a", "del-b", "del-c"] {
        let s = Session::new(
            id.into(),
            "/tmp".into(),
            "test-model".into(),
            storage.clone(),
        )
        .await?;
        s.write_items(vec![assistant("hi")]).await?;
    }

    // Deleting an existing session reports true and removes just that one.
    assert!(storage.delete_session("del-b").await?);
    let remaining = storage.list_sessions(ListSessions { limit: 100 }).await?;
    let ids: Vec<&str> = remaining.iter().map(|s| s.session_id.as_str()).collect();
    assert_eq!(remaining.len(), 2);
    assert!(ids.contains(&"del-a"));
    assert!(ids.contains(&"del-c"));
    assert!(!ids.contains(&"del-b"));

    // Deleting an already-gone id reports false rather than erroring, which is
    // what lets the bulk endpoint treat a stale client list as a no-op.
    assert!(!storage.delete_session("del-b").await?);
    Ok(())
}

/// The dashboard shell is served verbatim via `include_str!`. Guard that the
/// inline session search stays wired in, since the surrounding React bundle has
/// no source in-repo to lean on.
#[test]
fn dashboard_shell_embeds_search() {
    let html = include_str!("../src/gateway/channels/http/static/dashboard/index.html");
    // Inline search bar injected into the "Connected sessions" section.
    assert!(html.contains("id=\"evot-search-input\""));
    assert!(html.contains("evot-search-results"));
    assert!(html.contains("Connected sessions"));
    assert!(html.contains("/api/sessions"));
    // Results link to the SPA session route and highlight matches.
    assert!(html.contains("/sessions/"));
    assert!(html.contains("evot-hl"));
    // Pagination/sort controls are hidden while a query is active.
    assert!(html.contains(".evot-pagination, .evot-sort"));
    // Detail page exposes a copyable `evot --resume <id>` command.
    assert!(html.contains("evot-resume-cmd"));
    assert!(html.contains("evot --resume "));
    // Manage mode: multi-select + confirmed bulk delete via the API.
    assert!(html.contains("evot-manage-btn"));
    assert!(html.contains("evot-delete-selected"));
    assert!(html.contains("/api/sessions/delete"));
    assert!(html.contains("cannot be undone"));
}

/// The hand-written /chat page exposes the same search affordance.
#[test]
fn chat_page_embeds_search_overlay() {
    let html = include_str!("../src/gateway/channels/http/static/index.html");
    assert!(html.contains("id=\"searchOverlay\""));
    assert!(html.contains("/api/sessions"));
    assert!(html.contains("function highlight"));
}
