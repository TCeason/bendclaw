use crate::types::TranscriptEntry;
use crate::types::TranscriptItem;
use crate::types::UsageSummary;

const SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:\n\n";

pub fn compact_summary_item(summary: &str) -> TranscriptItem {
    TranscriptItem::User {
        text: format!("{SUMMARY_PREFIX}{summary}"),
        content: vec![],
    }
}

pub fn resolve_context_entries(entries: &[TranscriptEntry]) -> Vec<(u64, TranscriptItem)> {
    let last_control = entries.iter().rposition(|e| is_control_point(&e.item));

    match last_control {
        Some(idx) => match &entries[idx].item {
            TranscriptItem::Compact {
                summary,
                first_kept_seq,
                ..
            } => {
                let compact_seq = entries[idx].seq;
                let mut items = vec![(0, compact_summary_item(summary))];
                for entry in entries {
                    if entry.seq >= *first_kept_seq
                        && entry.seq < compact_seq
                        && entry.item.is_context_item()
                    {
                        // These entries are retained from before the compact
                        // control point. Their assistant usage reflects the
                        // pre-compaction context and must not be reused as a
                        // fresh pre-prompt baseline.
                        items.push((entry.seq, clear_assistant_usage(entry.item.clone())));
                    }
                }
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

fn clear_assistant_usage(item: TranscriptItem) -> TranscriptItem {
    match item {
        TranscriptItem::Assistant {
            content,
            stop_reason,
            model,
            provider,
            timestamp,
            error_message,
            ..
        } => TranscriptItem::Assistant {
            content,
            stop_reason,
            usage: UsageSummary::default(),
            model,
            provider,
            timestamp,
            error_message,
        },
        other => other,
    }
}
