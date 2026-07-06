//! NAPI host bridge: routes engine tool delegations back to the TypeScript CLI.
//!
//! This generalizes the former bespoke `ask.rs`. Any host-owned tool (ask_user,
//! plan, or a user extension's tool) the engine invokes is forwarded to JS as a
//! `host_tool_call` event and answered via `NapiRun::respond_host_tool`.
//!
//! Multiple host tools can be in flight at once (parallel tool execution), so
//! responders are keyed by `tool_call_id` rather than a single slot.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use evot_engine::host::HostBridge;
use evot_engine::host::HostError;
use evot_engine::host::HostToolCall;
use evot_engine::host::HostToolResponse;
use evot_engine::host::HostToolSpec;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

/// Result carried back from JS for a single host tool call.
type HostResult = std::result::Result<HostToolResponse, String>;

/// Map of in-flight tool calls awaiting a JS response, keyed by tool_call_id.
pub(crate) type HostResponders = Arc<Mutex<HashMap<String, oneshot::Sender<HostResult>>>>;

/// The engine-facing bridge. Serializes each call as a synthetic event, parks a
/// responder, and blocks until JS answers.
pub(crate) struct NapiHostBridge {
    event_tx: tokio_mpsc::UnboundedSender<String>,
    responders: HostResponders,
}

impl NapiHostBridge {
    pub(crate) fn new(
        event_tx: tokio_mpsc::UnboundedSender<String>,
        responders: HostResponders,
    ) -> Self {
        Self {
            event_tx,
            responders,
        }
    }
}

#[async_trait]
impl HostBridge for NapiHostBridge {
    async fn execute_tool(
        &self,
        call: HostToolCall,
    ) -> std::result::Result<HostToolResponse, HostError> {
        let event_json = serde_json::json!({
            "kind": "host_tool_call",
            "payload": {
                "tool_name": call.tool_name,
                "tool_call_id": call.tool_call_id,
                "arguments": call.arguments,
            }
        });
        let json_str = serde_json::to_string(&event_json)
            .map_err(|e| HostError::Failed(format!("serialize host_tool_call: {e}")))?;

        let (resp_tx, resp_rx) = oneshot::channel::<HostResult>();
        {
            let mut guard = self.responders.lock().await;
            guard.insert(call.tool_call_id.clone(), resp_tx);
        }

        if self.event_tx.send(json_str).is_err() {
            // Receiver gone — clean up the parked responder.
            self.responders.lock().await.remove(&call.tool_call_id);
            return Err(HostError::Closed);
        }

        match resp_rx.await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(msg)) => Err(HostError::Failed(msg)),
            Err(_) => Err(HostError::Closed),
        }
    }
}

/// Parse the host tool specs the TS side registered, passed as a JSON array.
///
/// A `None` or empty payload yields no specs — the run then carries only
/// engine-owned built-in tools.
pub(crate) fn parse_host_tool_specs(json: Option<&str>) -> Result<Vec<HostToolSpec>, String> {
    match json {
        None => Ok(Vec::new()),
        Some(s) if s.trim().is_empty() => Ok(Vec::new()),
        Some(s) => serde_json::from_str::<Vec<HostToolSpec>>(s)
            .map_err(|e| format!("parse host tool specs: {e}")),
    }
}
