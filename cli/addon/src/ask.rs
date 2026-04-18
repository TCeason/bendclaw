use std::sync::Arc;

use evot_engine::tools::AskUserFn;
use evot_engine::tools::AskUserRequest;
use evot_engine::tools::AskUserResponse;
use futures::FutureExt;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

/// Shared slot for the oneshot sender that unblocks the `AskUserFn` callback.
pub(crate) type AskResponder =
    Arc<Mutex<Option<oneshot::Sender<std::result::Result<AskUserResponse, String>>>>>;

/// Build an `AskUserFn` that bridges Rust ↔ JS:
/// 1. Serializes the `AskUserRequest` as a JSON event and sends it via `ask_event_tx`
/// 2. Stores a oneshot sender in `ask_responder`
/// 3. Blocks until JS calls `respond_ask_user()` which sends the answer back
pub(crate) fn build_ask_fn(
    ask_event_tx: tokio_mpsc::UnboundedSender<String>,
    ask_responder: AskResponder,
) -> AskUserFn {
    Arc::new(move |request: AskUserRequest| {
        let tx = ask_event_tx.clone();
        let responder = ask_responder.clone();
        (async move {
            // Serialize the request as a synthetic event JSON
            let questions_value = match serde_json::to_value(&request.questions) {
                Ok(v) => v,
                Err(e) => return Err(format!("serialize ask_user questions: {e}")),
            };
            let event_json = serde_json::json!({
                "kind": "ask_user",
                "payload": { "questions": questions_value }
            });
            let json_str = match serde_json::to_string(&event_json) {
                Ok(s) => s,
                Err(e) => return Err(format!("serialize ask_user event: {e}")),
            };

            // Create a oneshot channel for the response
            let (resp_tx, resp_rx) =
                oneshot::channel::<std::result::Result<AskUserResponse, String>>();

            // Store the sender so respond_ask_user() can find it
            {
                let mut guard = responder.lock().await;
                *guard = Some(resp_tx);
            }

            // Send the event to the JS side (will be picked up by next())
            if let Err(e) = tx.send(json_str) {
                return Err(format!("send ask_user event: {e}"));
            }

            // Block until JS responds
            match resp_rx.await {
                Ok(result) => result,
                Err(_) => Err("ask_user response channel closed".into()),
            }
        })
        .boxed()
    })
}
