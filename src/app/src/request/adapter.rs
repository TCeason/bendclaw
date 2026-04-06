use serde_json::json;

use super::event::AssistantBlock;
use super::event::AssistantPayload;
use super::event::RequestFinishedPayload;
use super::event::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;
use crate::storage::model::ToolCallRecord;
use crate::storage::model::TranscriptEntry;
use crate::storage::model::TranscriptItem;

// ---------------------------------------------------------------------------
// AgentEvent -> RunEvent
// ---------------------------------------------------------------------------

pub fn map_agent_event_to_run_event(
    event: &bend_engine::AgentEvent,
    run_id: &str,
    session_id: &str,
    turn: u32,
) -> Option<RunEvent> {
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
        bend_engine::AgentEvent::MessageEnd { message } => {
            let payload = build_assistant_payload(message);
            (RunEventKind::AssistantCompleted, payload)
        }
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
        } => {
            let content = extract_content_text(&result.content);
            let payload = serde_json::to_value(ToolResultPayload {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content,
                is_error: *is_error,
            })
            .unwrap_or(json!({}));
            (RunEventKind::ToolFinished, payload)
        }
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
        // AgentEnd is handled separately in executor (needs duration/transcript_count)
        bend_engine::AgentEvent::AgentEnd { .. } => return None,
        // Skip these — no useful app-level event
        bend_engine::AgentEvent::MessageStart { .. }
        | bend_engine::AgentEvent::TurnEnd { .. }
        | bend_engine::AgentEvent::MessageUpdate { .. } => return None,
    };

    Some(RunEvent::new(
        run_id.to_string(),
        session_id.to_string(),
        turn,
        kind,
        payload,
    ))
}

pub fn build_run_finished_event(
    run_id: &str,
    session_id: &str,
    turn: u32,
    messages: &[bend_engine::AgentMessage],
    duration_ms: u64,
) -> RunEvent {
    let text = extract_last_assistant_text(messages);
    let usage = total_usage(messages);
    let transcript_count = messages.len();

    let payload = serde_json::to_value(RequestFinishedPayload {
        text,
        usage,
        turn_count: turn,
        duration_ms,
        transcript_count,
    })
    .unwrap_or(json!({}));

    RunEvent::new(
        run_id.to_string(),
        session_id.to_string(),
        turn,
        RunEventKind::RunFinished,
        payload,
    )
}

// ---------------------------------------------------------------------------
// AgentMessage -> TranscriptItem
// ---------------------------------------------------------------------------

pub fn map_agent_message_to_transcript_item(
    message: &bend_engine::AgentMessage,
) -> Option<TranscriptItem> {
    match message {
        bend_engine::AgentMessage::Llm(bend_engine::Message::User { content, .. }) => {
            let text = extract_content_text(content);
            Some(TranscriptItem::User { text })
        }
        bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant { content, .. }) => {
            let mut text = String::new();
            let mut thinking: Option<String> = None;
            let mut tool_calls: Vec<ToolCallRecord> = Vec::new();

            for block in content {
                match block {
                    bend_engine::Content::Text { text: t } => {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                    bend_engine::Content::Thinking { thinking: t, .. } => {
                        thinking = Some(t.clone());
                    }
                    bend_engine::Content::ToolCall {
                        id,
                        name,
                        arguments,
                    } => {
                        tool_calls.push(ToolCallRecord {
                            id: id.clone(),
                            name: name.clone(),
                            input: arguments.clone(),
                        });
                    }
                    _ => {}
                }
            }

            Some(TranscriptItem::Assistant {
                text,
                thinking,
                tool_calls,
            })
        }
        bend_engine::AgentMessage::Llm(bend_engine::Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        }) => {
            let text = extract_content_text(content);
            Some(TranscriptItem::ToolResult {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content: text,
                is_error: *is_error,
            })
        }
        bend_engine::AgentMessage::Extension(ext) => Some(TranscriptItem::Extension {
            kind: ext.kind.clone(),
            data: ext.data.clone(),
        }),
    }
}

pub fn map_agent_messages_to_transcript_items(
    messages: &[bend_engine::AgentMessage],
) -> Vec<TranscriptItem> {
    messages
        .iter()
        .filter_map(map_agent_message_to_transcript_item)
        .collect()
}

pub fn map_agent_messages_to_transcript_entries(
    session_id: &str,
    run_id: Option<String>,
    turn: u32,
    messages: &[bend_engine::AgentMessage],
) -> Vec<TranscriptEntry> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(idx, msg)| {
            let item = map_agent_message_to_transcript_item(msg)?;
            Some(TranscriptEntry::new(
                session_id.to_string(),
                run_id.clone(),
                idx as u64 + 1,
                turn,
                item,
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// TranscriptItem -> AgentMessage (for restoring context)
// ---------------------------------------------------------------------------

pub fn transcript_items_to_agent_messages(
    items: &[TranscriptItem],
) -> Vec<bend_engine::AgentMessage> {
    items
        .iter()
        .filter_map(transcript_item_to_agent_message)
        .collect()
}

fn transcript_item_to_agent_message(item: &TranscriptItem) -> Option<bend_engine::AgentMessage> {
    match item {
        TranscriptItem::User { text } => Some(bend_engine::AgentMessage::Llm(
            bend_engine::Message::user(text.clone()),
        )),
        TranscriptItem::Assistant {
            text,
            thinking,
            tool_calls,
        } => {
            let mut content: Vec<bend_engine::Content> = Vec::new();
            if let Some(t) = thinking {
                content.push(bend_engine::Content::Thinking {
                    thinking: t.clone(),
                    signature: None,
                });
            }
            if !text.is_empty() {
                content.push(bend_engine::Content::Text { text: text.clone() });
            }
            for tc in tool_calls {
                content.push(bend_engine::Content::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.input.clone(),
                });
            }
            Some(bend_engine::AgentMessage::Llm(
                bend_engine::Message::Assistant {
                    content,
                    stop_reason: bend_engine::StopReason::Stop,
                    model: String::new(),
                    provider: String::new(),
                    usage: bend_engine::Usage::default(),
                    timestamp: bend_engine::types::now_ms(),
                    error_message: None,
                },
            ))
        }
        TranscriptItem::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
        } => Some(bend_engine::AgentMessage::Llm(
            bend_engine::Message::ToolResult {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content: vec![bend_engine::Content::Text {
                    text: content.clone(),
                }],
                is_error: *is_error,
                timestamp: bend_engine::types::now_ms(),
            },
        )),
        TranscriptItem::System { text } => Some(bend_engine::AgentMessage::Extension(
            bend_engine::ExtensionMessage::new("system", serde_json::json!({ "text": text })),
        )),
        TranscriptItem::Extension { kind, data } => Some(bend_engine::AgentMessage::Extension(
            bend_engine::ExtensionMessage::new(kind.clone(), data.clone()),
        )),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn extract_first_user_text(items: &[TranscriptItem]) -> Option<String> {
    items.iter().find_map(|item| {
        if let TranscriptItem::User { text } = item {
            if !text.trim().is_empty() {
                return Some(text.clone());
            }
        }
        None
    })
}

pub fn extract_assistant_text(message: &bend_engine::AgentMessage) -> String {
    if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant { content, .. }) = message
    {
        return extract_content_text(content);
    }
    String::new()
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

fn build_assistant_payload(message: &bend_engine::AgentMessage) -> serde_json::Value {
    let (content, usage) = match message {
        bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
            content, usage, ..
        }) => {
            let blocks: Vec<AssistantBlock> = content
                .iter()
                .filter_map(|c| match c {
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
            let usage_val = serde_json::to_value(usage).ok();
            (blocks, usage_val)
        }
        _ => (vec![], None),
    };

    serde_json::to_value(AssistantPayload { content, usage }).unwrap_or(json!({}))
}

fn extract_last_assistant_text(messages: &[bend_engine::AgentMessage]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|m| {
            if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                content, ..
            }) = m
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

fn total_usage(messages: &[bend_engine::AgentMessage]) -> serde_json::Value {
    let mut input: u64 = 0;
    let mut output: u64 = 0;
    for msg in messages {
        if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant { usage, .. }) = msg {
            input += usage.input;
            output += usage.output;
        }
    }
    json!({ "input": input, "output": output })
}
