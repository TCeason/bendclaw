use anyhow::Result;
use axum::http::header::CONTENT_TYPE;
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use axum::Router;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::providers::openai::OpenAIProvider;
use bendclaw::llm::stream::StreamEvent;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_stream::StreamExt;

#[tokio::test]
async fn openai_stream_falls_back_to_json_response() -> Result<()> {
    let (base_url, shutdown_tx) = start_openai_json_server().await?;
    let provider = OpenAIProvider::new(&format!("{base_url}/v1"), "test-key")?;
    let mut stream = provider.chat_stream("gpt-4.5", &[ChatMessage::user("hi")], &[], 1.0);

    let mut text = String::new();
    let mut done = None;
    let mut usage = None;

    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ContentDelta(chunk) => text.push_str(&chunk),
            StreamEvent::Usage(tokens) => usage = Some(tokens),
            StreamEvent::Done {
                finish_reason,
                provider,
                model,
            } => done = Some((finish_reason, provider, model)),
            StreamEvent::Error(error) => anyhow::bail!("unexpected stream error: {error}"),
            _ => {}
        }
    }

    let _ = shutdown_tx.send(());

    assert_eq!(text, "Hello from JSON fallback.");
    let usage = usage.expect("expected usage");
    assert_eq!(usage.prompt_tokens, 12);
    assert_eq!(usage.completion_tokens, 5);

    let done = done.expect("expected done event");
    assert_eq!(done.0, "stop");
    assert_eq!(done.1.as_deref(), Some("openai"));
    assert_eq!(done.2.as_deref(), Some("gpt-4.5"));
    Ok(())
}

#[tokio::test]
async fn openai_stream_falls_back_to_trailing_json_body() -> Result<()> {
    let (base_url, shutdown_tx) = start_openai_fake_stream_server().await?;
    let provider = OpenAIProvider::new(&format!("{base_url}/v1"), "test-key")?;
    let mut stream = provider.chat_stream("gpt-4.5", &[ChatMessage::user("hi")], &[], 1.0);

    let mut text = String::new();
    let mut done = None;

    while let Some(event) = stream.next().await {
        match event {
            StreamEvent::ContentDelta(chunk) => text.push_str(&chunk),
            StreamEvent::Done {
                finish_reason,
                provider,
                model,
            } => done = Some((finish_reason, provider, model)),
            StreamEvent::Error(error) => anyhow::bail!("unexpected stream error: {error}"),
            _ => {}
        }
    }

    let _ = shutdown_tx.send(());

    assert_eq!(text, "Hello from trailing JSON body.");
    let done = done.expect("expected done event");
    assert_eq!(done.0, "stop");
    assert_eq!(done.1.as_deref(), Some("openai"));
    assert_eq!(done.2.as_deref(), Some("gpt-4.5"));
    Ok(())
}

#[tokio::test]
async fn openai_stream_surfaces_stream_error_payload() -> Result<()> {
    let (base_url, shutdown_tx) = start_openai_stream_error_server().await?;
    let provider = OpenAIProvider::new(&format!("{base_url}/v1"), "test-key")?;
    let mut stream = provider.chat_stream("gpt-4.5", &[ChatMessage::user("hi")], &[], 1.0);

    let mut got_error = None;

    while let Some(event) = stream.next().await {
        if let StreamEvent::Error(error) = event {
            got_error = Some(error);
            break;
        }
    }

    let _ = shutdown_tx.send(());

    let error = got_error.expect("expected stream error");
    assert!(error.contains("not supported"));
    assert!(error.contains("server_error"));
    Ok(())
}

async fn start_openai_json_server() -> Result<(String, oneshot::Sender<()>)> {
    async fn handler() -> impl IntoResponse {
        let body = serde_json::json!({
            "id": "chatcmpl_test",
            "object": "chat.completion",
            "model": "gpt-4.5",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Hello from JSON fallback."
                }
            }],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 5,
                "total_tokens": 17
            }
        });

        (
            [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
            Json(body),
        )
    }

    let app = Router::new().route("/v1/chat/completions", post(handler));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    Ok((format!("http://{}", addr), shutdown_tx))
}

async fn start_openai_stream_error_server() -> Result<(String, oneshot::Sender<()>)> {
    async fn handler() -> impl IntoResponse {
        let body = r#"data: {"error":{"message":"Upstream error 400: {\"detail\":\"The 'gpt-4.5' model is not supported when using Codex with a ChatGPT account.\"}","type":"server_error"}}

"#;

        (
            [(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
            body,
        )
    }

    let app = Router::new().route("/v1/chat/completions", post(handler));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    Ok((format!("http://{}", addr), shutdown_tx))
}

async fn start_openai_fake_stream_server() -> Result<(String, oneshot::Sender<()>)> {
    async fn handler() -> impl IntoResponse {
        let body = serde_json::json!({
            "id": "chatcmpl_test",
            "object": "chat.completion",
            "model": "gpt-4.5",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Hello from trailing JSON body."
                }
            }]
        });

        (
            [(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
            body.to_string(),
        )
    }

    let app = Router::new().route("/v1/chat/completions", post(handler));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    Ok((format!("http://{}", addr), shutdown_tx))
}
