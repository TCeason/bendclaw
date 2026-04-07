use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptItem {
    User {
        text: String,
    },
    Assistant {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
        #[serde(default)]
        tool_calls: Vec<ToolCallRecord>,
        stop_reason: String,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
    },
    System {
        text: String,
    },
    Extension {
        kind: String,
        data: serde_json::Value,
    },
    Compact {
        messages: Vec<TranscriptItem>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub session_id: String,
    pub run_id: Option<String>,
    pub seq: u64,
    pub turn: u32,
    pub item: TranscriptItem,
    pub created_at: String,
}

impl TranscriptEntry {
    pub fn new(
        session_id: String,
        run_id: Option<String>,
        seq: u64,
        turn: u32,
        item: TranscriptItem,
    ) -> Self {
        Self {
            session_id,
            run_id,
            seq,
            turn,
            item,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTranscriptEntries {
    pub session_id: String,
    pub run_id: Option<String>,
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}
