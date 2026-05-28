//! JSON output compaction for command outputs that are raw JSON.

use serde_json::Value;

use super::super::filter::CmdCtx;
use super::super::filter::CmdFilter;
use super::super::filter::Stream;

const MAX_DEPTH: usize = 4;
const MAX_STRING_CHARS: usize = 80;
const MAX_ARRAY_ITEMS: usize = 5;
const MAX_OBJECT_KEYS: usize = 20;

/// Only compact JSON outputs larger than this threshold.
/// JSON compaction is lossy (truncates strings, drops array items, limits depth),
/// so we only apply it to large outputs where token savings outweigh information loss.
/// 32KB ≈ 8-10k tokens — well within context budget; above this the savings matter.
const MIN_BYTES_TO_COMPACT: usize = 32 * 1024;

pub struct JsonFilter;

impl CmdFilter for JsonFilter {
    fn id(&self) -> &'static str {
        "json"
    }

    fn apply(&self, _ctx: &CmdCtx<'_>, stream: Stream, text: &str) -> Option<String> {
        if stream != Stream::Stdout || !looks_like_json(text) {
            return None;
        }
        if text.len() < MIN_BYTES_TO_COMPACT {
            return None;
        }

        let value: Value = serde_json::from_str(text).ok()?;
        let compacted = compact_json(&value, 0, MAX_DEPTH);
        if compacted.len() < text.len() {
            Some(compacted)
        } else {
            None
        }
    }
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

fn compact_json(value: &Value, depth: usize, max_depth: usize) -> String {
    let indent = "  ".repeat(depth);

    if depth > max_depth {
        return format!("{}...", indent);
    }

    match value {
        Value::Null => format!("{}null", indent),
        Value::Bool(b) => format!("{}{}", indent, b),
        Value::Number(n) => format!("{}{}", indent, n),
        Value::String(s) => compact_string(s, &indent),
        Value::Array(arr) => compact_array(arr, depth, max_depth, &indent),
        Value::Object(map) => {
            if map.is_empty() {
                format!("{}{{}}", indent)
            } else {
                let mut lines = vec![format!("{}{{", indent)];
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();

                for (i, key) in keys.iter().enumerate() {
                    if i >= MAX_OBJECT_KEYS {
                        lines.push(format!("{}  ... +{} more keys", indent, keys.len() - i));
                        break;
                    }

                    let val = &map[*key];
                    if is_simple(val) {
                        let val_str = compact_json(val, 0, max_depth);
                        lines.push(format!("{}  {}: {}", indent, key, val_str.trim()));
                    } else {
                        lines.push(format!("{}  {}:", indent, key));
                        lines.push(compact_json(val, depth + 1, max_depth));
                    }
                }
                lines.push(format!("{}}}", indent));
                lines.join("\n")
            }
        }
    }
}

fn compact_string(s: &str, indent: &str) -> String {
    let rendered = if s.chars().count() > MAX_STRING_CHARS {
        let end = char_boundary_after_chars(s, MAX_STRING_CHARS - 3);
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    };
    let quoted = serde_json::to_string(&rendered).unwrap_or_else(|_| "\"\"".to_string());
    format!("{}{}", indent, quoted)
}

fn compact_array(arr: &[Value], depth: usize, max_depth: usize, indent: &str) -> String {
    if arr.is_empty() {
        return format!("{}[]", indent);
    }

    if arr.len() > MAX_ARRAY_ITEMS {
        let first = compact_json(&arr[0], depth + 1, max_depth);
        return format!("{}[{}, ... +{} more]", indent, first.trim(), arr.len() - 1);
    }

    let items: Vec<String> = arr
        .iter()
        .map(|v| compact_json(v, depth + 1, max_depth))
        .collect();
    if arr.iter().all(is_simple) {
        let inline: Vec<&str> = items.iter().map(|s| s.trim()).collect();
        format!("{}[{}]", indent, inline.join(", "))
    } else {
        let mut lines = vec![format!("{}[", indent)];
        for item in &items {
            lines.push(format!("{},", item));
        }
        lines.push(format!("{}]", indent));
        lines.join("\n")
    }
}

fn is_simple(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn char_boundary_after_chars(s: &str, count: usize) -> usize {
    s.char_indices()
        .nth(count)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
