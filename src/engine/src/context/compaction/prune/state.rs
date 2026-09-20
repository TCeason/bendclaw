//! The state one batch of the call round reads, fitted to the judge's
//! request budget. Token counts come from the judge: it knows its tokenizer.

use std::collections::HashMap;

use super::candidates::compact_json;
use super::candidates::Candidate;
use crate::judge::Judge;
use crate::types::AgentMessage;
use crate::types::Content;
use crate::types::Message;

/// Characters of a tool result shown to the judge before abridging.
const RESULT_EXCERPT_CHARS: usize = 1_600;

/// The state one batch reads: the task, then the stretch of conversation from
/// the batch's first call to its last result — verbatim, tool results
/// excerpted — fitted into `budget` tokens, oldest lines abridged first. A
/// trailing line says how much conversation follows, so "later superseded"
/// is answerable. Returns the state and the message range it rendered.
pub(super) fn render(
    messages: &[AgentMessage],
    batch: &[Candidate],
    task: &str,
    budget: usize,
    judge: &dyn Judge,
) -> (String, usize, usize) {
    let tags: HashMap<&str, usize> = batch
        .iter()
        .enumerate()
        .map(|(n, c)| (c.call_id.as_str(), n))
        .collect();
    let from = batch.iter().map(|c| c.call_index).min().unwrap_or(0);
    let to = batch
        .iter()
        .map(|c| c.result_index.unwrap_or(c.call_index))
        .max()
        .unwrap_or(from)
        .min(messages.len().saturating_sub(1));
    let mut entries: Vec<Entry> = messages[from..=to]
        .iter()
        .filter_map(|message| render_entry(message, &tags))
        .collect();
    let after = messages.len().saturating_sub(to + 1);
    let head = format!("{task}\n\n# Conversation (messages {from}–{to} of {}; the calls to judge are tagged t0, t1, …)", messages.len());
    let tail = if after > 0 {
        format!("[… {after} later messages follow, up to the present …]")
    } else {
        "[end of conversation]".to_string()
    };
    fit(
        &mut entries,
        budget.saturating_sub(judge.estimate_tokens(&head) + judge.estimate_tokens(&tail)),
        &|text| judge.estimate_tokens(text),
    );
    let body: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
    (format!("{head}\n{}\n{tail}", body.join("\n")), from, to)
}

/// One rendered message. `judged` marks the entries the batch is asked
/// about; they keep at least a head when the state is fitted.
struct Entry {
    text: String,
    judged: bool,
}

fn render_entry(message: &AgentMessage, tags: &HashMap<&str, usize>) -> Option<Entry> {
    let AgentMessage::Llm(message) = message else {
        return None;
    };
    match message {
        Message::User { content, .. } => Some(Entry {
            text: format!("user: {}", text_of(content)),
            judged: false,
        }),
        Message::Assistant { content, .. } => {
            let mut lines = Vec::new();
            let mut judged = false;
            let text = text_of(content);
            if !text.is_empty() {
                lines.push(format!("assistant: {text}"));
            }
            for block in content {
                if let Content::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } = block
                {
                    let tag = tags.get(id.as_str());
                    judged |= tag.is_some();
                    let tag = tag.map(|n| format!("t{n} ")).unwrap_or_default();
                    lines.push(format!("{tag}call {name} {}", compact_json(arguments)));
                }
            }
            (!lines.is_empty()).then(|| Entry {
                text: lines.join("\n"),
                judged,
            })
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => {
            let tag = tags.get(tool_call_id.as_str());
            let judged = tag.is_some();
            let tag = tag.map(|n| format!("t{n} ")).unwrap_or_default();
            let body = text_of(content);
            let status = if *is_error { "error" } else { "ok" };
            Some(Entry {
                text: format!(
                    "{tag}{tool_name} -> {status}\n{}",
                    abridge(&body, RESULT_EXCERPT_CHARS)
                ),
                judged,
            })
        }
    }
}

/// Abridge oldest entries first until the state fits: long entries to head +
/// tail, then whole old entries to a note. The newest quarter keeps its text;
/// judged entries keep at least a head.
fn fit(entries: &mut [Entry], budget: usize, tokens: &dyn Fn(&str) -> usize) {
    let total = |entries: &[Entry]| entries.iter().map(|e| tokens(&e.text) + 1).sum::<usize>();
    if total(entries) <= budget {
        return;
    }
    let abridgeable = entries.len() - entries.len() / 4;
    for cap in [1000usize, 400, 160, 60] {
        for entry in entries.iter_mut().take(abridgeable) {
            if entry.text.chars().count() > cap {
                entry.text = abridge(&entry.text, cap);
            }
        }
        if total(entries) <= budget {
            return;
        }
    }
    for cap in [400usize, 160, 60] {
        for entry in entries.iter_mut() {
            if entry.text.chars().count() > cap {
                entry.text = abridge(&entry.text, cap);
            }
        }
        if total(entries) <= budget {
            return;
        }
    }
    for entry in entries.iter_mut().take(abridgeable) {
        if !entry.judged {
            entry.text = format!("[… {} chars omitted …]", entry.text.chars().count());
        }
    }
}

pub(super) fn text_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn abridge(text: &str, cap: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= cap {
        return text.to_string();
    }
    let head: String = chars[..cap * 2 / 3].iter().collect();
    let tail: String = chars[chars.len() - cap / 3..].iter().collect();
    format!("{head} […] {tail}")
}
