use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Sse;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use futures::stream::Stream;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

use crate::conf::bend_provider_kind;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::BendclawError;
use crate::error::Result;

const INDEX_HTML: &str = include_str!("index.html");

struct AppState {
    agent: Mutex<Option<bend_agent::Agent>>,
    llm: LlmConfig,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
}

pub async fn start(conf: Config) -> Result<()> {
    let llm = conf.active_llm();
    let state = Arc::new(AppState {
        agent: Mutex::new(None),
        llm,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/new", post(new_session_handler))
        .route("/api/chat", post(chat_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", conf.server.host, conf.server.port);
    tracing::info!("bendclaw server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| BendclawError::Run(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| BendclawError::Run(format!("server error: {e}")))?;

    Ok(())
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn new_session_handler(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    if let Some(agent) = state.agent.lock().await.take() {
        agent.close().await;
    }
    Json(StatusResponse {
        status: "ok".into(),
    })
}

async fn chat_handler(
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
            let opts = bend_agent::AgentOptions {
                provider: Some(bend_provider_kind(&state.llm.provider)),
                model: Some(state.llm.model.clone()),
                api_key: Some(state.llm.api_key.clone()),
                base_url: state.llm.base_url.clone(),
                max_turns: Some(20),
                ..Default::default()
            };
            match bend_agent::Agent::new(opts).await {
                Ok(a) => agent = Some(a),
                Err(e) => {
                    let _ = tx
                        .send(sse_event("error", &json!({"message": e.to_string()})))
                        .await;
                    let _ = tx.send(sse_event("done", &json!(null))).await;
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
            let sse_data = map_sdk_to_sse(&event, &start);
            for data in sse_data {
                if tx.send(data).await.is_err() {
                    break;
                }
            }
        }

        let _ = handle.await;
        let _ = tx.send(sse_event("done", &json!(null))).await;

        *state.agent.lock().await = Some(agent);
    });

    ReceiverStream::new(rx)
}

fn map_sdk_to_sse(
    event: &bend_agent::SDKMessage,
    start: &std::time::Instant,
) -> Vec<std::result::Result<axum::response::sse::Event, Infallible>> {
    let mut events = Vec::new();

    match event {
        bend_agent::SDKMessage::Assistant { message, .. } => {
            for block in &message.content {
                match block {
                    bend_agent::ContentBlock::Text { text } if !text.is_empty() => {
                        events.push(sse_event("text", &json!({"text": text})));
                    }
                    bend_agent::ContentBlock::ToolUse { id, name, input } => {
                        events.push(sse_event(
                            "tool_use",
                            &json!({"id": id, "name": name, "input": input}),
                        ));
                    }
                    bend_agent::ContentBlock::Thinking { thinking, .. } if !thinking.is_empty() => {
                        events.push(sse_event("thinking", &json!({"thinking": thinking})));
                    }
                    _ => {}
                }
            }
        }
        bend_agent::SDKMessage::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            events.push(sse_event(
                "tool_result",
                &json!({
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error,
                }),
            ));
        }
        bend_agent::SDKMessage::Result {
            usage,
            num_turns,
            cost_usd,
            ..
        } => {
            events.push(sse_event(
                "result",
                &json!({
                    "num_turns": num_turns,
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cost": cost_usd,
                    "duration_ms": start.elapsed().as_millis() as u64,
                }),
            ));
        }
        bend_agent::SDKMessage::Error { message } => {
            events.push(sse_event("error", &json!({"message": message})));
        }
        bend_agent::SDKMessage::PartialMessage { text } => {
            events.push(sse_event("text", &json!({"text": text})));
        }
        _ => {}
    }

    events
}

fn sse_event(
    event_type: &str,
    data: &serde_json::Value,
) -> std::result::Result<axum::response::sse::Event, Infallible> {
    let payload = json!({"type": event_type, "data": data});
    match serde_json::to_string(&payload) {
        Ok(json) => Ok(axum::response::sse::Event::default().data(json)),
        Err(_) => Ok(axum::response::sse::Event::default().data(String::new())),
    }
}
