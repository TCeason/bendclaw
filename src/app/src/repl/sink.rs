use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use super::render::build_run_summary;
use super::render::format_tool_input;
use super::render::print_tool_result;
use super::render::terminal_message_prefix;
use super::render::terminal_prefixed_writeln;
use super::render::terminal_write;
use super::render::terminal_writeln;
use super::render::Spinner;
use super::render::ToolCallSummary;
use super::render::DIM;
use super::render::RED;
use super::render::RESET;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::EventSink;
use crate::request::RequestFinishedPayload;
use crate::request::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct SinkState {
    pub assistant_open: bool,
    pub assistant_prefixed: bool,
    pub streamed_assistant: bool,
    pub pending_tools: HashMap<String, ToolCallDisplay>,
    pub spinner: Option<Spinner>,
}

pub struct ToolCallDisplay {
    pub name: String,
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn finish_assistant_line(state: &mut SinkState) {
    if state.assistant_open {
        terminal_writeln("");
    }
    state.assistant_open = false;
    state.assistant_prefixed = false;
    state.streamed_assistant = false;
}

pub fn stop_spinner(state: &mut SinkState) {
    if let Some(mut spinner) = state.spinner.take() {
        spinner.stop();
    }
}

pub fn update_spinner(state: &mut SinkState, message: &str) {
    if let Some(spinner) = state.spinner.as_mut() {
        spinner.update(message);
    } else {
        state.spinner = Some(Spinner::start(message));
    }
}

// ---------------------------------------------------------------------------
// ReplSink
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ReplSink {
    state: Mutex<SinkState>,
}

#[async_trait]
impl EventSink for ReplSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| crate::error::BendclawError::Cli("sink state lock poisoned".into()))?;

        match &event.kind {
            RunEventKind::RunStarted => {
                state.assistant_open = false;
                state.assistant_prefixed = false;
                state.streamed_assistant = false;
                state.spinner = Some(Spinner::start("Thinking..."));
            }
            RunEventKind::TurnStarted => {
                if state.spinner.is_none() {
                    state.spinner = Some(Spinner::start("Thinking..."));
                }
            }
            RunEventKind::AssistantCompleted => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                    for block in payload.content {
                        match block {
                            AssistantBlock::Text { text } => {
                                if state.streamed_assistant {
                                    terminal_writeln("");
                                } else if !text.trim().is_empty() {
                                    terminal_prefixed_writeln(&text);
                                }
                                state.assistant_open = false;
                                state.assistant_prefixed = false;
                                state.streamed_assistant = false;
                            }
                            AssistantBlock::ToolCall { id, name, input } => {
                                finish_assistant_line(&mut state);
                                state.pending_tools.insert(id, ToolCallDisplay {
                                    name: name.clone(),
                                    summary: format_tool_input(&input),
                                });
                                super::render::print_tool_call(&name, &input);
                            }
                            AssistantBlock::Thinking { .. } => {}
                        }
                    }
                }
            }
            RunEventKind::ToolFinished => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<ToolResultPayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    let tool_call = state.pending_tools.remove(&payload.tool_call_id).map(|tc| {
                        ToolCallSummary {
                            name: tc.name,
                            summary: tc.summary,
                        }
                    });
                    print_tool_result(&payload, tool_call.as_ref());
                }
            }
            RunEventKind::AssistantDelta => {
                stop_spinner(&mut state);
                if let Some(delta) = event.payload.get("delta").and_then(|v| v.as_str()) {
                    if !state.assistant_prefixed {
                        terminal_message_prefix();
                        state.assistant_prefixed = true;
                    }
                    terminal_write(delta);
                    state.assistant_open = true;
                    state.streamed_assistant = true;
                }
            }
            RunEventKind::ToolStarted => {
                if let Some(name) = event.payload.get("tool_name").and_then(|v| v.as_str()) {
                    update_spinner(&mut state, &format!("Running {name}..."));
                }
            }
            RunEventKind::ToolProgress => {
                if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                    update_spinner(&mut state, text);
                }
            }
            RunEventKind::Error => {
                stop_spinner(&mut state);
                if let Some(message) = event.payload.get("message").and_then(|v| v.as_str()) {
                    finish_assistant_line(&mut state);
                    terminal_writeln(&format!("{RED}error:{RESET} {message}"));
                }
            }
            RunEventKind::RunFinished => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<RequestFinishedPayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    let summary = build_run_summary(&payload);
                    if !summary.is_empty() {
                        terminal_writeln(&format!("{DIM}{summary}{RESET}"));
                    }
                }
            }
        }

        Ok(())
    }
}
