use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use axum::extract::Path;
use axum::extract::State;
use axum::extract::WebSocketUpgrade;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;

use super::trace;
use crate::agent::Agent;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;

// mission-control SPA assets (compiled React bundle)
const SPA_HTML: &str = include_str!("../static/dashboard/index.html");
const SPA_JS: &str = include_str!("../static/dashboard/assets/index.js");
const SPA_CSS: &str = include_str!("../static/dashboard/assets/index.css");
const SPA_LOGO: &str = include_str!("../static/dashboard/assets/logo.svg");

#[derive(Clone)]
pub struct DashboardState {
    pub agent: Arc<Agent>,
}

pub fn dashboard_router(agent: Arc<Agent>) -> Router {
    let state = DashboardState { agent };
    Router::new()
        // SPA static assets
        .route("/assets/index.js", get(spa_js))
        .route("/assets/index.css", get(spa_css))
        .route("/assets/logo.svg", get(spa_logo))
        // Session trace (per-LLM-call spans with tool calls)
        .route("/api/session/{id}/events", get(api_events))
        .route("/api/session/{id}/events/{seq}", get(api_event_detail))
        .route("/api/session/{id}/activity", get(api_activity))
        // Live data streams
        .route("/ws", get(ws_sessions))
        .route("/ws/logs", get(ws_logs))
        // SPA entry — serve index.html for the app shell and client-side routes
        .route("/", get(spa_index))
        .route("/sessions/{id}", get(spa_index))
        // Native trace viewer page (self-contained, not part of the SPA bundle)
        .route("/sessions/{id}/trace", get(trace_page))
        .with_state(state)
}

// --- SPA static assets ---

async fn spa_index() -> Html<&'static str> {
    Html(SPA_HTML)
}

async fn spa_js() -> impl IntoResponse {
    ([("content-type", "text/javascript")], SPA_JS)
}

async fn spa_css() -> impl IntoResponse {
    ([("content-type", "text/css")], SPA_CSS)
}

async fn spa_logo() -> impl IntoResponse {
    ([("content-type", "image/svg+xml")], SPA_LOGO)
}

// --- WebSocket: live sessions + vitals ---

async fn ws_sessions(
    ws: WebSocketUpgrade,
    State(state): State<DashboardState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_sessions_loop(socket, state))
}

async fn ws_sessions_loop(mut socket: WebSocket, state: DashboardState) {
    // Push a sessions + vitals snapshot every 2s until the client disconnects.
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    loop {
        let payload = sessions_vitals_payload(&state).await;
        let text = match serde_json::to_string(&payload) {
            Ok(t) => t,
            Err(_) => break,
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
        tokio::select! {
            _ = ticker.tick() => {}
            msg = socket.recv() => {
                // Stop when the client closes or the stream ends.
                if matches!(msg, None | Some(Ok(Message::Close(_))) | Some(Err(_))) {
                    break;
                }
            }
        }
    }
}

async fn sessions_vitals_payload(state: &DashboardState) -> serde_json::Value {
    let sessions = match state
        .agent
        .storage()
        .list_sessions(ListSessions { limit: 50 })
        .await
    {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("dashboard: failed to list sessions: {e}");
            Vec::new()
        }
    };

    let sessions_json: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.session_id,
                "cwd": s.cwd,
                "input_tokens": s.total_input_tokens,
                "output_tokens": s.total_output_tokens,
                // Accurate count when persisted; fall back to turns for sessions
                // saved before span_count existed.
                "span_count": s.span_count.unwrap_or(s.turns),
                "started_at": s.created_at,
                "last_request_at": s.updated_at,
                "parent_id": serde_json::Value::Null,
                "fork_turn_idx": serde_json::Value::Null,
            })
        })
        .collect();

    let m = super::metrics::collect();
    serde_json::json!({
        "sessions": sessions_json,
        "vitals": {
            "cpu_percent": m.cpu_percent,
            "cpu_available": m.cpu_available,
            "ram_used": (m.ram_used_mb * 1_048_576.0) as u64,
            "ram_total": (m.ram_total_mb * 1_048_576.0) as u64,
            "disk_total": m.disk_total_gb * 1_073_741_824,
            "disk_used": m.disk_total_gb.saturating_sub(m.disk_available_gb) * 1_073_741_824,
        }
    })
}

// --- WebSocket: server logs ---

async fn ws_logs(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(ws_logs_loop)
}

async fn ws_logs_loop(mut socket: WebSocket) {
    // No structured log bus is wired in yet; keep the socket open with a
    // heartbeat so the SPA log panel connects cleanly instead of erroring.
    let mut ticker = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                if matches!(msg, None | Some(Ok(Message::Close(_))) | Some(Err(_))) {
                    break;
                }
            }
        }
    }
}

// --- API: session trace (per-LLM-call spans) ---

const TRACE_HTML: &str = include_str!("../static/trace/index.html");

async fn load_entries(state: &DashboardState, id: &str) -> Vec<crate::types::TranscriptEntry> {
    state
        .agent
        .storage()
        .list_entries(ListTranscriptEntries {
            session_id: id.to_string(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await
        .unwrap_or_default()
}

async fn api_events(
    State(state): State<DashboardState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let entries = load_entries(&state, &id).await;
    Json(trace::project_spans(&entries))
}

async fn api_event_detail(
    State(state): State<DashboardState>,
    Path((id, seq)): Path<(String, u64)>,
) -> impl IntoResponse {
    let entries = load_entries(&state, &id).await;
    match trace::project_span_detail(&entries, seq) {
        Some(detail) => Json(detail).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "span not found").into_response(),
    }
}

async fn api_activity(
    State(state): State<DashboardState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let meta = state.agent.storage().get_session(&id).await.ok().flatten();
    let entries = load_entries(&state, &id).await;
    Json(trace::project_activity(&entries, meta.as_ref())).into_response()
}

async fn trace_page() -> Html<&'static str> {
    Html(TRACE_HTML)
}
