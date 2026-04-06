use serde_json::json;

use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

pub fn request_started_event(run_id: &str, session_id: &str) -> RunEvent {
    RunEvent::new(
        run_id.to_string(),
        session_id.to_string(),
        0,
        RunEventKind::RunStarted,
        json!({}),
    )
}
