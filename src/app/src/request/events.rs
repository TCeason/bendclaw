use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serde_json::Value;

use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

#[derive(Clone, Copy)]
pub struct RunEventContext<'a> {
    run_id: &'a str,
    session_id: &'a str,
    turn: u32,
}

impl<'a> RunEventContext<'a> {
    pub fn new(run_id: &'a str, session_id: &'a str, turn: u32) -> Self {
        Self {
            run_id,
            session_id,
            turn,
        }
    }

    pub fn started(&self) -> RunEvent {
        self.with_turn(0).event(RunEventKind::RunStarted, json!({}))
    }

    pub fn finished(&self, messages: &[bend_engine::AgentMessage], duration_ms: u64) -> RunEvent {
        self.event(
            RunEventKind::RunFinished,
            serde_json::to_value(RequestFinishedPayload::from_messages(
                messages,
                self.turn,
                duration_ms,
            ))
            .unwrap_or(json!({})),
        )
    }

    pub fn map(&self, event: &bend_engine::AgentEvent) -> Option<RunEvent> {
        let (kind, payload) = match event {
            bend_engine::AgentEvent::AgentStart => return None,
            bend_engine::AgentEvent::TurnStart => (RunEventKind::TurnStarted, json!({})),
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::Text { delta },
                ..
            } => (RunEventKind::AssistantDelta, json!({ "delta": delta })),
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::Thinking { delta },
                ..
            } => (
                RunEventKind::AssistantDelta,
                json!({ "thinking_delta": delta }),
            ),
            bend_engine::AgentEvent::MessageEnd { message } => (
                RunEventKind::AssistantCompleted,
                serde_json::to_value(AssistantPayload::from(message)).unwrap_or(json!({})),
            ),
            bend_engine::AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => (
                RunEventKind::ToolStarted,
                json!({ "tool_call_id": tool_call_id, "tool_name": tool_name, "args": args }),
            ),
            bend_engine::AgentEvent::ToolExecutionUpdate {
                tool_call_id,
                tool_name,
                partial_result,
            } => {
                let text = extract_content_text(&partial_result.content);
                (
                    RunEventKind::ToolProgress,
                    json!({ "tool_call_id": tool_call_id, "tool_name": tool_name, "text": text }),
                )
            }
            bend_engine::AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
            } => (
                RunEventKind::ToolFinished,
                serde_json::to_value(ToolResultPayload::from_result(
                    tool_call_id,
                    tool_name,
                    result,
                    *is_error,
                ))
                .unwrap_or(json!({})),
            ),
            bend_engine::AgentEvent::ProgressMessage {
                tool_call_id,
                tool_name,
                text,
            } => (
                RunEventKind::ToolProgress,
                json!({ "tool_call_id": tool_call_id, "tool_name": tool_name, "text": text }),
            ),
            bend_engine::AgentEvent::InputRejected { reason } => {
                (RunEventKind::Error, json!({ "message": reason }))
            }
            bend_engine::AgentEvent::AgentEnd { .. } => return None,
            bend_engine::AgentEvent::MessageStart { .. }
            | bend_engine::AgentEvent::TurnEnd { .. }
            | bend_engine::AgentEvent::MessageUpdate { .. } => return None,
        };

        Some(self.event(kind, payload))
    }

    fn with_turn(self, turn: u32) -> Self {
        Self { turn, ..self }
    }

    fn event(self, kind: RunEventKind, payload: Value) -> RunEvent {
        RunEvent::new(
            self.run_id.to_string(),
            self.session_id.to_string(),
            self.turn,
            kind,
            payload,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestFinishedPayload {
    pub text: String,
    pub usage: Value,
    pub turn_count: u32,
    pub duration_ms: u64,
    pub transcript_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantPayload {
    pub content: Vec<AssistantBlock>,
    pub usage: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantBlock {
    Text {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    Thinking {
        text: String,
    },
}

pub fn payload_as<T: DeserializeOwned>(payload: &Value) -> Option<T> {
    serde_json::from_value(payload.clone()).ok()
}

impl From<&bend_engine::AgentMessage> for AssistantPayload {
    fn from(message: &bend_engine::AgentMessage) -> Self {
        let (content, usage) = match message {
            bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                content,
                usage,
                ..
            }) => {
                let blocks: Vec<AssistantBlock> = content
                    .iter()
                    .filter_map(|block| match block {
                        bend_engine::Content::Text { text } => {
                            Some(AssistantBlock::Text { text: text.clone() })
                        }
                        bend_engine::Content::Thinking { thinking, .. } => {
                            Some(AssistantBlock::Thinking {
                                text: thinking.clone(),
                            })
                        }
                        bend_engine::Content::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(AssistantBlock::ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: arguments.clone(),
                        }),
                        _ => None,
                    })
                    .collect();
                let usage = serde_json::to_value(usage).ok();
                (blocks, usage)
            }
            _ => (vec![], None),
        };

        Self { content, usage }
    }
}

impl ToolResultPayload {
    fn from_result(
        tool_call_id: &str,
        tool_name: &str,
        result: &bend_engine::ToolResult,
        is_error: bool,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            content: extract_content_text(&result.content),
            is_error,
        }
    }
}

impl RequestFinishedPayload {
    fn from_messages(
        messages: &[bend_engine::AgentMessage],
        turn_count: u32,
        duration_ms: u64,
    ) -> Self {
        Self {
            text: extract_last_assistant_text(messages),
            usage: total_usage(messages),
            turn_count,
            duration_ms,
            transcript_count: messages.len(),
        }
    }
}

fn extract_content_text(content: &[bend_engine::Content]) -> String {
    content
        .iter()
        .filter_map(|c| {
            if let bend_engine::Content::Text { text } = c {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_last_assistant_text(messages: &[bend_engine::AgentMessage]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|message| {
            if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                content, ..
            }) = message
            {
                let text = extract_content_text(content);
                if !text.is_empty() {
                    return Some(text);
                }
            }
            None
        })
        .unwrap_or_default()
}

fn total_usage(messages: &[bend_engine::AgentMessage]) -> Value {
    let mut input: u64 = 0;
    let mut output: u64 = 0;

    for message in messages {
        if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant { usage, .. }) =
            message
        {
            input += usage.input;
            output += usage.output;
        }
    }

    json!({ "input": input, "output": output })
}
