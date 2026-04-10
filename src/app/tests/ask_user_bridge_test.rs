//! Tests for the ask_user channel bridge — the glue between engine and REPL.
//!
//! These test the channel roundtrip without any terminal IO.

use std::sync::Arc;

use bend_engine::tools::AskUserFn;
use bend_engine::tools::AskUserOption;
use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

/// Build the same ask_fn + channel pair that repl.rs creates.
fn make_bridge() -> (
    AskUserFn,
    mpsc::UnboundedReceiver<(AskUserRequest, oneshot::Sender<AskUserResponse>)>,
) {
    let (ask_tx, ask_rx) = mpsc::unbounded_channel();
    let ask_fn: AskUserFn = Arc::new(move |request| {
        let tx = ask_tx.clone();
        Box::pin(async move {
            let (resp_tx, resp_rx) = oneshot::channel();
            tx.send((request, resp_tx)).map_err(|e| e.to_string())?;
            resp_rx.await.map_err(|e| e.to_string())
        })
    });
    (ask_fn, ask_rx)
}

fn sample_request() -> AskUserRequest {
    AskUserRequest {
        question: "Which approach?".into(),
        options: vec![
            AskUserOption {
                label: "Option A (Recommended)".into(),
                description: "First choice".into(),
            },
            AskUserOption {
                label: "Option B".into(),
                description: "Second choice".into(),
            },
        ],
    }
}

#[tokio::test]
async fn bridge_selected_roundtrip() {
    let (ask_fn, mut ask_rx) = make_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    // Simulate the REPL side receiving and responding
    let (request, responder) = ask_rx.recv().await.unwrap();
    assert_eq!(request.question, "Which approach?");
    assert_eq!(request.options.len(), 2);
    responder
        .send(AskUserResponse::Selected("Option A (Recommended)".into()))
        .unwrap();

    let result = handle.await.unwrap().unwrap();
    assert_eq!(
        result,
        AskUserResponse::Selected("Option A (Recommended)".into())
    );
}

#[tokio::test]
async fn bridge_custom_roundtrip() {
    let (ask_fn, mut ask_rx) = make_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    let (_request, responder) = ask_rx.recv().await.unwrap();
    responder
        .send(AskUserResponse::Custom("Use SQLite".into()))
        .unwrap();

    let result = handle.await.unwrap().unwrap();
    assert_eq!(result, AskUserResponse::Custom("Use SQLite".into()));
}

#[tokio::test]
async fn bridge_skipped_roundtrip() {
    let (ask_fn, mut ask_rx) = make_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    let (_request, responder) = ask_rx.recv().await.unwrap();
    responder.send(AskUserResponse::Skipped).unwrap();

    let result = handle.await.unwrap().unwrap();
    assert_eq!(result, AskUserResponse::Skipped);
}

#[tokio::test]
async fn bridge_responder_dropped_returns_error() {
    let (ask_fn, mut ask_rx) = make_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    // Receive but drop the responder without sending
    let (_request, _responder) = ask_rx.recv().await.unwrap();
    drop(_responder);

    let result = handle.await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn bridge_receiver_dropped_returns_error() {
    let (ask_fn, ask_rx) = make_bridge();

    // Drop the receiver before sending
    drop(ask_rx);

    let result = (ask_fn)(sample_request()).await;
    assert!(result.is_err());
}
