use crate::types::TranscriptEntry;
use crate::types::TranscriptItem;

const SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:\n\n";

pub fn compact_summary_text(summary: &str) -> String {
    format!("{SUMMARY_PREFIX}{summary}")
}

pub fn compact_summary_item(summary: &str) -> TranscriptItem {
    TranscriptItem::User {
        text: compact_summary_text(summary),
        content: vec![],
    }
}

pub fn resolve_context_entries(entries: &[TranscriptEntry]) -> Vec<(u64, TranscriptItem)> {
    let last_control = entries.iter().rposition(|e| is_control_point(&e.item));

    match last_control {
        Some(idx) => match &entries[idx].item {
            TranscriptItem::Compact { messages, .. } => {
                let mut items = messages
                    .iter()
                    .filter(|item| item.is_context_item())
                    .cloned()
                    .map(|item| (0, item))
                    .collect::<Vec<_>>();
                for entry in &entries[idx + 1..] {
                    if entry.item.is_context_item() {
                        items.push((entry.seq, entry.item.clone()));
                    }
                }
                items
            }
            TranscriptItem::Marker { messages, .. } => {
                let mut items: Vec<(u64, TranscriptItem)> = messages
                    .iter()
                    .filter(|item| item.is_context_item())
                    .cloned()
                    .map(|item| (0, item))
                    .collect();
                for entry in &entries[idx + 1..] {
                    if entry.item.is_context_item() {
                        items.push((entry.seq, entry.item.clone()));
                    }
                }
                items
            }
            _ => unreachable!("control point predicate and match are inconsistent"),
        },
        None => entries
            .iter()
            .filter(|entry| entry.item.is_context_item())
            .map(|entry| (entry.seq, entry.item.clone()))
            .collect(),
    }
}

pub fn resolve_context_items(entries: &[TranscriptEntry]) -> Vec<TranscriptItem> {
    resolve_context_entries(entries)
        .into_iter()
        .map(|(_, item)| item)
        .collect()
}

/// Resolve the exact Engine context at the latest control point.
pub fn resolve_engine_context(entries: &[TranscriptEntry]) -> Vec<evot_engine::AgentMessage> {
    let last_control = entries
        .iter()
        .rposition(|entry| is_control_point(&entry.item));
    let mut messages = match last_control {
        Some(index) => match &entries[index].item {
            TranscriptItem::Compact {
                engine_messages, ..
            } => engine_messages.clone(),
            TranscriptItem::Marker { messages, .. } => {
                crate::agent::run::convert::into_agent_messages(messages)
            }
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let start = last_control
        .map(|index| index.saturating_add(1))
        .unwrap_or(0);
    messages.extend(crate::agent::run::convert::into_agent_messages(
        &entries[start..]
            .iter()
            .filter(|entry| entry.item.is_context_item())
            .map(|entry| entry.item.clone())
            .collect::<Vec<_>>(),
    ));
    evot_engine::migrate_legacy_responses_tool_ids(&mut messages);
    messages
}

pub fn resolve_snapshot_at(entries: &[TranscriptEntry], target_seq: u64) -> Vec<TranscriptItem> {
    let scoped: Vec<TranscriptEntry> = entries
        .iter()
        .filter(|entry| entry.seq <= target_seq)
        .cloned()
        .collect();
    resolve_context_items(&scoped)
}

fn is_control_point(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::Compact { .. } | TranscriptItem::Marker { .. }
    )
}
