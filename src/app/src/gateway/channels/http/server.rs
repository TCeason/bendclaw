use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use parking_lot::RwLock;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::agent::Agent;
use crate::agent::QueryRequest;
use crate::agent::SubmitOutcome;
use crate::conf::Config;
use crate::conf::SettingsUpdate;
use crate::error::EvotError;
use crate::error::Result;
use crate::gateway::channels::http::stream;

const INDEX_HTML: &str = include_str!("static/index.html");
const SETTINGS_HTML: &str = include_str!("static/settings/index.html");

/// `0` means no limit in the storage layer. The dashboard owns pagination, so
/// `/api/sessions` should return every saved session rather than an arbitrary
/// first page.
const SESSION_SEARCH_LIMIT: usize = 0;

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

#[derive(Deserialize)]
struct ToggleFavoriteRequest {
    id: String,
}

pub struct Server {
    agent: Arc<Agent>,
    /// The live, mutable runtime config. Shared so the settings API can read a
    /// masked snapshot and apply edits in place, then persist to the env file.
    config: Arc<RwLock<Config>>,
}

impl Server {
    pub fn new(agent: Arc<Agent>, config: Config) -> Arc<Self> {
        Arc::new(Self {
            agent,
            config: Arc::new(RwLock::new(config)),
        })
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
            .route(
                "/api/favorites",
                get(|State(server): State<Arc<Server>>| async move { server.favorites().await }),
            )
            .route(
                "/api/favorites/toggle",
                post(
                    |State(server): State<Arc<Server>>,
                     Json(req): Json<ToggleFavoriteRequest>| async move {
                        server.toggle_favorite(req).await
                    },
                ),
            )
            .route(
                "/settings",
                get(|| async { Html(SETTINGS_HTML) }),
            )
            .route(
                "/api/settings",
                get(|State(server): State<Arc<Server>>| async move { server.get_settings() })
                    .post(
                        |State(server): State<Arc<Server>>,
                         Json(req): Json<SettingsUpdate>| async move {
                            server.update_settings(req)
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

    /// Returns the current LLM provider + Feishu config with secrets masked, so
    /// the settings page can render the form without ever exposing raw keys.
    ///
    /// Reloads from the env file on disk first, so the page reflects edits made
    /// outside the dashboard (e.g. a hand-edited `evot.env`) rather than a stale
    /// in-memory snapshot. This also keeps the next save's "leave blank to keep"
    /// behavior anchored to the real on-disk secrets.
    fn get_settings(&self) -> impl IntoResponse {
        if let Err(e) = self.reload_config_from_disk() {
            // Fall back to the in-memory config rather than failing the page; a
            // transient read error shouldn't blank out the settings UI.
            tracing::warn!("settings: reload from disk failed, serving cached config: {e}");
        }
        let snapshot = crate::conf::settings_snapshot(&self.config.read());
        Json(snapshot)
    }

    /// Re-read the env file from disk and replace the shared config. Uses the
    /// path the process was started with so a custom `--env-file` is honored.
    fn reload_config_from_disk(&self) -> Result<()> {
        let env_path = self.config.read().env_file_path.clone();
        let path_arg = env_path.to_str();
        let fresh = Config::load_with_env_file(path_arg)?;
        *self.config.write() = fresh;
        Ok(())
    }

    /// Validate, persist to the env file, and hot-apply a settings update.
    ///
    /// LLM changes take effect on the next message (the agent's `LlmConfig` is
    /// rebuilt here). Feishu changes are persisted but require a restart to
    /// re-spawn the channel; the response carries `feishu_restart_required` so
    /// the UI can surface that.
    fn update_settings(&self, update: SettingsUpdate) -> impl IntoResponse {
        let feishu_changed = update.feishu.is_some();
        match self.apply_and_persist(update) {
            Ok(()) => (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "feishu_restart_required": feishu_changed,
                    "settings": crate::conf::settings_snapshot(&self.config.read()),
                })),
            )
                .into_response(),
            Err(e) => (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            )
                .into_response(),
        }
    }

    /// Apply the update to the shared config, persist it to the env file, and
    /// push the rebuilt active LLM into the running agent. Holds the config
    /// write lock for the whole operation so concurrent edits cannot interleave.
    fn apply_and_persist(&self, update: SettingsUpdate) -> Result<()> {
        let mut config = self.config.write();
        crate::conf::apply_settings(&mut config, &update)?;
        // Surface resolution errors (e.g. missing key) before writing the file.
        let llm = config.active_llm()?;
        let env_path = config.env_file_path.clone();
        // Generate the managed block from the resolved config so secrets and
        // every field live inside it — the block is the single source of truth.
        let groups = crate::conf::config_to_env_groups(&config);
        crate::conf::env_writer::write_grouped(&env_path, &groups)?;
        self.agent.set_llm(llm);
        Ok(())
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
        let mut deleted_ids: Vec<String> = Vec::new();
        let mut failed: Vec<String> = Vec::new();
        for id in &req.ids {
            // Stop an in-flight run before removing files, otherwise the run's
            // next transcript write would recreate the directory.
            self.agent.abort_run(id);
            match self.agent.delete_session(id).await {
                Ok(true) => {
                    deleted += 1;
                    deleted_ids.push(id.clone());
                }
                Ok(false) => {
                    // Already gone: still prune a stale favorite entry for this id.
                    deleted_ids.push(id.clone());
                }
                Err(e) => {
                    tracing::warn!(session_id = %id, "delete failed: {e}");
                    failed.push(id.clone());
                }
            }
        }
        if !deleted_ids.is_empty() {
            if let Err(e) = self.agent.remove_favorites(&deleted_ids).await {
                tracing::warn!("failed to prune deleted sessions from favorites: {e}");
            }
        }
        Json(serde_json::json!({
            "deleted": deleted,
            "requested": req.ids.len(),
            "failed": failed,
        }))
        .into_response()
    }

    /// Returns the set of favorited session ids so the dashboard can pin and
    /// sort them on load.
    async fn favorites(&self) -> impl IntoResponse {
        match self.agent.list_favorites().await {
            Ok(ids) => Json(serde_json::json!({ "ids": ids })).into_response(),
            Err(e) => {
                tracing::warn!("dashboard: failed to list favorites: {e}");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to list favorites",
                )
                    .into_response()
            }
        }
    }

    /// Toggles a session's favorite state and reports the new value.
    async fn toggle_favorite(&self, req: ToggleFavoriteRequest) -> impl IntoResponse {
        match self.agent.toggle_favorite(&req.id).await {
            Ok(favorited) => {
                Json(serde_json::json!({ "id": req.id, "favorited": favorited })).into_response()
            }
            Err(e) => {
                tracing::warn!(session_id = %req.id, "toggle favorite failed: {e}");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to toggle favorite",
                )
                    .into_response()
            }
        }
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
