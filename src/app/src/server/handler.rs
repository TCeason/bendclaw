use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::Json;
use futures::stream::Stream;
use serde::Deserialize;
use serde::Serialize;
use tokio_stream::wrappers::ReceiverStream;

use crate::agent::build_agent_options;
use crate::server::server::AppState;
use crate::server::stream;

const INDEX_HTML: &str = include_str!("index.html");

#[derive(Deserialize)]
pub(crate) struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    status: String,
}

pub(crate) async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub(crate) async fn new_session(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    if let Some(agent) = state.agent.lock().await.take() {
        agent.close().await;
    }
    Json(StatusResponse {
        status: "ok".into(),
    })
}

pub(crate) async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let stream = chat_stream(state, req.message);
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}

fn chat_stream(
    state: Arc<AppState>,
    message: String,
) -> impl Stream<Item = std::result::Result<axum::response::sse::Event, Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    tokio::spawn(async move {
        let start = std::time::Instant::now();

        let mut agent = state.agent.lock().await.take();

        if agent.is_none() {
            let opts = build_agent_options(&state.llm, None, Some(20));
            match bend_agent::Agent::new(opts).await {
                Ok(a) => agent = Some(a),
                Err(e) => {
                    let _ = tx.send(stream::error_event(e.to_string())).await;
                    let _ = tx.send(stream::done_event()).await;
                    return;
                }
            }
        }

        let mut agent = match agent {
            Some(a) => a,
            None => return,
        };

        let (mut sdk_rx, handle) = agent.query(&message).await;

        while let Some(event) = sdk_rx.recv().await {
            let sse_data = stream::map_sdk_message(&event, &start);
            for data in sse_data {
                if tx.send(data).await.is_err() {
                    break;
                }
            }
        }

        let _ = handle.await;
        let _ = tx.send(stream::done_event()).await;

        *state.agent.lock().await = Some(agent);
    });

    ReceiverStream::new(rx)
}
