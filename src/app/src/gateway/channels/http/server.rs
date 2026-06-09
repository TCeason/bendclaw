use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::SubmitOutcome;
use crate::error::EvotError;
use crate::error::Result;
use crate::gateway::channels::http::stream;

const INDEX_HTML: &str = include_str!("static/index.html");

/// Cap on sessions returned by `/api/sessions`. Matches the terminal resume
/// pool size and keeps the full-text payload bounded.
const SESSION_SEARCH_LIMIT: usize = 100;

/// Cap on ids accepted per `/api/sessions/delete` call. Bounds the work a single
/// request can trigger; the UI never selects more than the listed pool anyway.
const MAX_DELETE_BATCH: usize = 200;

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Deserialize)]
struct DeleteSessionsRequest {
    ids: Vec<String>,
}

pub struct Server {
    agent: Arc<Agent>,
}

impl Server {
    pub fn new(agent: Arc<Agent>) -> Arc<Self> {
        Arc::new(Self { agent })
    }

    pub async fn start(self: Arc<Self>, host: String, port: u16) -> Result<()> {
        let addr = format!("{host}:{port}");
        tracing::info!(stage = "server", status = "listening", addr = %addr);

        // Auto-open mission control in browser
        let url = format!("http://{addr}/");
        let _ = std::thread::spawn(move || {
            // Small delay to ensure server is ready
            std::thread::sleep(std::time::Duration::from_millis(300));
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open").arg(&url).spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", &url])
                    .spawn();
            }
        });

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| EvotError::Run(format!("failed to bind {addr}: {e}")))?;

        axum::serve(listener, self.router())
            .await
            .map_err(|e| EvotError::Run(format!("server error: {e}")))?;

        Ok(())
    }

    pub fn router(self: Arc<Self>) -> Router {
        let dashboard = super::dashboard::dashboard_router(self.agent.clone());
        Router::new()
            .route(
                "/chat",
                get(|State(server): State<Arc<Server>>| async move { server.index().await }),
            )
            .route(
                "/api/chat",
                post(
                    |State(server): State<Arc<Server>>, Json(req): Json<ChatRequest>| async move {
                        server.chat(req).await
                    },
                ),
            )
            .route(
                "/api/sessions",
                get(|State(server): State<Arc<Server>>| async move { server.sessions().await }),
            )
            .route(
                "/api/sessions/delete",
                post(
                    |State(server): State<Arc<Server>>,
                     Json(req): Json<DeleteSessionsRequest>| async move {
                        server.delete_sessions(req).await
                    },
                ),
            )
            .with_state(self)
            .merge(dashboard)
            .layer(CorsLayer::permissive())
    }

    async fn index(&self) -> Html<&'static str> {
        Html(INDEX_HTML)
    }

    /// Returns recent sessions, each with a flattened `search_text` field
    /// (id, title, cwd, model plus transcript snippets) so the chat UI can do
    /// client-side substring filtering and highlight matches, mirroring the
    /// terminal `/resume` selector.
    async fn sessions(&self) -> impl IntoResponse {
        match self
            .agent
            .list_sessions_with_text(SESSION_SEARCH_LIMIT)
            .await
        {
            Ok(sessions) => Json(sessions).into_response(),
            Err(e) => {
                tracing::warn!("chat: failed to list sessions with text: {e}");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to list sessions",
                )
                    .into_response()
            }
        }
    }

    /// Deletes the given sessions and reports how many were removed. Invalid or
    /// already-gone ids are skipped rather than failing the whole batch, so a
    /// concurrent deletion or a stale client list cannot wedge the request. Any
    /// active run for a session is aborted first so it cannot re-create the
    /// session directory and leave zombie state behind.
    async fn delete_sessions(&self, req: DeleteSessionsRequest) -> impl IntoResponse {
        if req.ids.len() > MAX_DELETE_BATCH {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("too many ids: {} (max {MAX_DELETE_BATCH})", req.ids.len()),
                })),
            )
                .into_response();
        }
        let mut deleted = 0usize;
        let mut failed: Vec<String> = Vec::new();
        for id in &req.ids {
            // Stop an in-flight run before removing files, otherwise the run's
            // next transcript write would recreate the directory.
            self.agent.abort_run(id);
            match self.agent.delete_session(id).await {
                Ok(true) => deleted += 1,
                Ok(false) => { /* already gone — treat as success, no-op */ }
                Err(e) => {
                    tracing::warn!(session_id = %id, "delete failed: {e}");
                    failed.push(id.clone());
                }
            }
        }
        Json(serde_json::json!({
            "deleted": deleted,
            "requested": req.ids.len(),
            "failed": failed,
        }))
        .into_response()
    }

    async fn chat(self: Arc<Self>, req: ChatRequest) -> impl IntoResponse {
        let stream = self.chat_stream(req.message, req.session_id);
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
    }

    fn chat_stream(
        self: Arc<Self>,
        message: String,
        session_id: Option<String>,
    ) -> impl futures::stream::Stream<
        Item = std::result::Result<axum::response::sse::Event, std::convert::Infallible>,
    > {
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let request = QueryRequest::text(&message)
                .session_id(session_id)
                .source("http");

            let drain_run = |mut query_run: crate::agent::Run, tx: tokio::sync::mpsc::Sender<_>| async move {
                while let Some(event) = query_run.next().await {
                    for sse in stream::map_run_event(&event) {
                        if tx.send(sse).await.is_err() {
                            break;
                        }
                    }
                }
            };

            match self.agent.submit(request).await {
                Ok(SubmitOutcome::Run(query_run)) => {
                    drain_run(query_run, tx.clone()).await;
                }
                Ok(SubmitOutcome::Command(text)) => {
                    let _ = tx.send(stream::text_event(&text)).await;
                }
                Err(e) => {
                    let _ = tx.send(stream::error_event(e.to_string())).await;
                }
            }

            let _ = tx.send(stream::done_event()).await;
        });

        tokio_stream::wrappers::ReceiverStream::new(rx)
    }
}
