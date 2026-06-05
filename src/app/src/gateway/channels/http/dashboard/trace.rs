//! Transcript → trace span projection.
//!
//! Mission control's session detail page shows a per-LLM-call trace: each
//! assistant response is one "span" carrying its tool calls and the tool
//! results they produced. This mirrors the eval trace viewer's span model
//! (evot-eval `web/public/js/span-renderer.js`) but is grounded entirely in
//! evot's own persisted transcript, so no placeholder data is involved.

use serde::Serialize;

use crate::types::TranscriptEntry;
use crate::types::TranscriptItem;
use crate::types::TranscriptStats;

/// One row in the trace list: a single assistant LLM call.
#[derive(Debug, Clone, Serialize)]
pub struct SpanSummary {
    pub seq: u64,
    pub model: String,
    pub stop_reason: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Wall-clock duration of the LLM call (ms); 0 when not recorded.
    pub duration_ms: u64,
    /// Number of messages sent to the model for this call.
    pub n_messages: usize,
    /// Number of tool calls the assistant requested in this span.
    pub n_tool_use: usize,
    /// Short preview of the assistant text (first line, trimmed).
    pub preview: String,
    /// Tool names requested, in order (for the collapsed summary line).
    pub tool_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// A content block within a span (assistant output, or an input-context block).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SpanBlock {
    Thinking {
        text: String,
    },
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Only appears in reconstructed input context, never in assistant output.
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// The tool result paired with a tool call from this span.
#[derive(Debug, Clone, Serialize)]
pub struct SpanToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
    /// Tool execution time (ms); 0 when not recorded.
    pub duration_ms: u64,
    /// Tokens the tool result contributed to context.
    pub result_tokens: usize,
}

/// Full detail for one span: assistant output blocks plus the tool results
/// produced before the next assistant call.
#[derive(Debug, Clone, Serialize)]
pub struct SpanDetail {
    pub seq: u64,
    pub model: String,
    pub stop_reason: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Wall-clock duration of the LLM call (ms).
    pub duration_ms: u64,
    /// Time to first token (ms).
    pub ttft_ms: u64,
    /// Context window size at the time of the call (tokens).
    pub context_window: usize,
    /// Number of messages sent to the model.
    pub n_messages: usize,
    /// System prompt size (tokens).
    pub system_prompt_tokens: usize,
    /// Full system prompt text sent to the model.
    pub system_prompt: String,
    /// Tool schemas sent to the model.
    pub tool_definitions: Vec<ToolDefView>,
    /// The user/tool input the assistant was reacting to (preview text).
    pub reacting_to: String,
    pub blocks: Vec<SpanBlock>,
    pub tool_results: Vec<SpanToolResult>,
    /// The cumulative conversation context sent to the model for this call,
    /// reconstructed from the transcript (respects compaction/markers). Used by
    /// the diff view to show how context grew between two requests.
    pub input_messages: Vec<InputBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// One content block of an input message, flattened with its message index and
/// role so the diff can show position + provenance.
#[derive(Debug, Clone, Serialize)]
pub struct InputBlock {
    /// Index of the source message within the input context.
    pub mi: usize,
    /// Message role: user / assistant / tool.
    pub role: String,
    #[serde(flatten)]
    pub block: SpanBlock,
}

/// A tool schema for the messages view (name + description + JSON Schema).
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefView {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

fn first_line_preview(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() > max {
        let truncated: String = line.chars().take(max).collect();
        format!("{truncated}\u{2026}")
    } else {
        line.to_string()
    }
}

/// LLM-call timing/sizing stats correlated to an assistant span. Lightweight:
/// no large payloads, so the span list can call this per row cheaply.
struct SpanStats {
    duration_ms: u64,
    ttft_ms: u64,
    context_window: usize,
    n_messages: usize,
    system_prompt_tokens: usize,
}

/// Correlate the persisted observability stats with the assistant entry at
/// `idx`. `LlmCallStarted` precedes the assistant message; `LlmCallCompleted`
/// follows it. Both sit adjacent to the assistant entry in the transcript, so
/// we scan a small window on each side and stop at the neighbouring assistant.
fn span_stats_for(entries: &[TranscriptEntry], idx: u64) -> SpanStats {
    let idx = idx as usize;
    let mut stats = SpanStats {
        duration_ms: 0,
        ttft_ms: 0,
        context_window: 0,
        n_messages: 0,
        system_prompt_tokens: 0,
    };

    // LlmCallStarted: nearest preceding stats before this assistant entry.
    for prev in entries[..idx].iter().rev() {
        match &prev.item {
            TranscriptItem::Assistant { .. } => break,
            item => {
                if let Some(TranscriptStats::LlmCallStarted(s)) =
                    TranscriptStats::try_from_item(item)
                {
                    stats.n_messages = s.message_count;
                    stats.system_prompt_tokens = s.system_prompt_tokens;
                    break;
                }
            }
        }
    }

    // LlmCallCompleted: nearest following stats before the next assistant entry.
    for next in &entries[idx + 1..] {
        match &next.item {
            TranscriptItem::Assistant { .. } => break,
            item => {
                if let Some(TranscriptStats::LlmCallCompleted(s)) =
                    TranscriptStats::try_from_item(item)
                {
                    if let Some(m) = s.metrics {
                        stats.duration_ms = m.duration_ms;
                        stats.ttft_ms = m.ttft_ms;
                    }
                    stats.context_window = s.context_window;
                    break;
                }
            }
        }
    }

    stats
}

/// Extract the system prompt + tool schemas from this span's `LlmCallStarted`
/// stats. Detail-only (the list never needs these large payloads), so it's
/// kept out of `span_stats_for`.
fn span_request_payload(entries: &[TranscriptEntry], idx: usize) -> (String, Vec<ToolDefView>) {
    for prev in entries[..idx].iter().rev() {
        match &prev.item {
            TranscriptItem::Assistant { .. } => break,
            item => {
                if let Some(TranscriptStats::LlmCallStarted(s)) =
                    TranscriptStats::try_from_item(item)
                {
                    let tools = s
                        .tool_definitions
                        .iter()
                        .map(|t| ToolDefView {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.parameters.clone(),
                        })
                        .collect();
                    return (s.system_prompt.clone(), tools);
                }
            }
        }
    }
    (String::new(), Vec::new())
}

/// Look up the persisted `ToolFinished` stats for a tool call id within this
/// span's window (between the assistant entry and the next assistant entry).
fn tool_finished_for(
    entries: &[TranscriptEntry],
    start_idx: usize,
    tool_call_id: &str,
) -> (u64, usize) {
    for next in &entries[start_idx + 1..] {
        if matches!(next.item, TranscriptItem::Assistant { .. }) {
            break;
        }
        if let Some(TranscriptStats::ToolFinished(s)) = TranscriptStats::try_from_item(&next.item) {
            if s.tool_call_id == tool_call_id {
                return (s.duration_ms, s.result_tokens);
            }
        }
    }
    (0, 0)
}

/// Project a session's transcript into a list of span summaries (one per
/// assistant LLM call), in chronological order.
pub fn project_spans(entries: &[TranscriptEntry]) -> Vec<SpanSummary> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| match &entry.item {
            TranscriptItem::Assistant {
                text,
                tool_calls,
                stop_reason,
                usage,
                model,
                error_message,
                ..
            } => {
                let stats = span_stats_for(entries, idx as u64);
                Some(SpanSummary {
                    seq: entry.seq,
                    model: model.clone(),
                    stop_reason: stop_reason.clone(),
                    input_tokens: usage.input,
                    output_tokens: usage.output,
                    duration_ms: stats.duration_ms,
                    n_messages: stats.n_messages,
                    n_tool_use: tool_calls.len(),
                    preview: first_line_preview(text, 120),
                    tool_names: tool_calls.iter().map(|t| t.name.clone()).collect(),
                    error_message: error_message.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

/// Reconstruct the input context sent to the model for the call at `seq`,
/// flattened into per-block entries for the diff view. Uses the same context
/// resolution as the live engine (honoring compaction/marker baselines), then
/// takes the snapshot *just before* this assistant turn — i.e. everything the
/// model saw as input, excluding this span's own response.
fn build_input_blocks(entries: &[TranscriptEntry], seq: u64) -> Vec<InputBlock> {
    let idx = match entries.iter().position(|e| e.seq == seq) {
        Some(i) => i,
        None => return Vec::new(),
    };
    let prior_seq = if idx == 0 { 0 } else { entries[idx - 1].seq };
    let items = crate::compact::context_view::resolve_snapshot_at(entries, prior_seq);

    let mut out = Vec::new();
    for (mi, item) in items.iter().enumerate() {
        match item {
            TranscriptItem::User { text, .. } => {
                if !text.trim().is_empty() {
                    out.push(InputBlock {
                        mi,
                        role: "user".into(),
                        block: SpanBlock::Text { text: text.clone() },
                    });
                }
            }
            TranscriptItem::Assistant {
                text,
                thinking,
                tool_calls,
                ..
            } => {
                if let Some(th) = thinking {
                    if !th.trim().is_empty() {
                        out.push(InputBlock {
                            mi,
                            role: "assistant".into(),
                            block: SpanBlock::Thinking { text: th.clone() },
                        });
                    }
                }
                if !text.trim().is_empty() {
                    out.push(InputBlock {
                        mi,
                        role: "assistant".into(),
                        block: SpanBlock::Text { text: text.clone() },
                    });
                }
                for tc in tool_calls {
                    out.push(InputBlock {
                        mi,
                        role: "assistant".into(),
                        block: SpanBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input: tc.input.clone(),
                        },
                    });
                }
            }
            TranscriptItem::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                out.push(InputBlock {
                    mi,
                    role: "tool".into(),
                    block: SpanBlock::ToolResult {
                        tool_use_id: tool_call_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    },
                });
            }
            _ => {}
        }
    }
    out
}

/// Build the full detail for the assistant span at `seq`, pairing it with the
/// tool results and the user/tool input that immediately precede/follow it.
pub fn project_span_detail(entries: &[TranscriptEntry], seq: u64) -> Option<SpanDetail> {
    let idx = entries.iter().position(|e| e.seq == seq)?;
    let entry = &entries[idx];
    let TranscriptItem::Assistant {
        text,
        thinking,
        tool_calls,
        stop_reason,
        usage,
        model,
        error_message,
        ..
    } = &entry.item
    else {
        return None;
    };

    // Assistant output blocks in display order: thinking, text, tool calls.
    let mut blocks = Vec::new();
    if let Some(th) = thinking {
        if !th.trim().is_empty() {
            blocks.push(SpanBlock::Thinking { text: th.clone() });
        }
    }
    if !text.trim().is_empty() {
        blocks.push(SpanBlock::Text { text: text.clone() });
    }
    for tc in tool_calls {
        blocks.push(SpanBlock::ToolUse {
            id: tc.id.clone(),
            name: tc.name.clone(),
            input: tc.input.clone(),
        });
    }

    // Tool results that follow this span, up to the next assistant call.
    let mut tool_results = Vec::new();
    for next in &entries[idx + 1..] {
        match &next.item {
            TranscriptItem::Assistant { .. } => break,
            TranscriptItem::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let (duration_ms, result_tokens) = tool_finished_for(entries, idx, tool_call_id);
                tool_results.push(SpanToolResult {
                    tool_use_id: tool_call_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                    duration_ms,
                    result_tokens,
                });
            }
            _ => {}
        }
    }

    // What the assistant was reacting to: the most recent user/tool input
    // before this span.
    let mut reacting_to = String::new();
    for prev in entries[..idx].iter().rev() {
        match &prev.item {
            TranscriptItem::User { text, .. } => {
                reacting_to = first_line_preview(text, 160);
                break;
            }
            TranscriptItem::ToolResult { tool_name, .. } => {
                reacting_to = format!("\u{2192} {tool_name} result");
                break;
            }
            TranscriptItem::Assistant { .. } => break,
            _ => {}
        }
    }

    let stats = span_stats_for(entries, idx as u64);
    let (system_prompt, tool_definitions) = span_request_payload(entries, idx);
    Some(SpanDetail {
        seq: entry.seq,
        model: model.clone(),
        stop_reason: stop_reason.clone(),
        input_tokens: usage.input,
        output_tokens: usage.output,
        cache_read: usage.cache_read,
        cache_write: usage.cache_write,
        duration_ms: stats.duration_ms,
        ttft_ms: stats.ttft_ms,
        context_window: stats.context_window,
        n_messages: stats.n_messages,
        system_prompt_tokens: stats.system_prompt_tokens,
        system_prompt,
        tool_definitions,
        reacting_to,
        blocks,
        tool_results,
        input_messages: build_input_blocks(entries, seq),
        error_message: error_message.clone(),
    })
}
