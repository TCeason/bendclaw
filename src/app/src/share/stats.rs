//! Explicit presentation of persisted stats. Never serialize raw stats payloads.
use serde_json::json;
use serde_json::Value;

pub fn tool_status(data: &Value) -> Option<Value> {
    let name = data.get("tool_name")?.as_str()?;
    let duration = data.get("duration_ms")?.as_u64()?;
    let failed = data
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // pi's built-in tool renderer has no duration field. Keep this compact
    // status separate without duplicating arguments, output, errors or usage.
    Some(
        json!({"type":"custom_message", "customType":"evot.tool", "display":true,
        "content":format!("{} {name} · {duration}ms", if failed { "✗" } else { "✓" })}),
    )
}

/// A judge prune, as one line a reader can follow: what was cut and which
/// user requests the judge considered the task at the time. Rounds that only
/// recorded verdicts without editing are bookkeeping and stay out.
pub fn prune_notice(data: &Value) -> Option<Value> {
    let applied = data.get("applied")?;
    let removed = applied.get("removed")?.as_u64()?;
    let truncated = applied.get("truncated")?.as_u64()?;
    if removed + truncated == 0 {
        return None;
    }
    let mut text = format!("✂ jev prune · removed {removed} · truncated {truncated}");
    if let (Some(before), Some(after)) = (
        applied.get("before_tokens").and_then(Value::as_u64),
        applied.get("after_tokens").and_then(Value::as_u64),
    ) {
        text.push_str(&format!(" · {} → {} tokens", tokens(before), tokens(after)));
    }
    if let Some(trigger) = applied.get("trigger").and_then(Value::as_str) {
        text.push_str(&format!(" · {}", match trigger {
            "savings" => "savings ≥ 20% of context",
            "cold_cache" => "cache cold",
            "before_compaction" => "before compaction",
            other => other,
        }));
    }
    let in_play: Vec<String> = data
        .get("decided")
        .and_then(|d| d.get("user_requests"))
        .and_then(Value::as_array)
        .map(|requests| {
            requests
                .iter()
                .filter(|r| r.get("in_play").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|r| {
                    let index = r.get("message_index")?.as_u64()?;
                    let request = r.get("text")?.as_str()?;
                    Some(format!("[{index}] {}", abridge(request, 60)))
                })
                .collect()
        })
        .unwrap_or_default();
    if !in_play.is_empty() {
        text.push_str(&format!("\nTask: {}", in_play.join(" · ")));
    }
    Some(
        json!({"type":"custom_message", "customType":"evot.prune", "display":true, "content":text}),
    )
}

fn tokens(n: u64) -> String {
    if n >= 1_000 {
        format!("{}k", n / 1_000)
    } else {
        n.to_string()
    }
}

fn abridge(text: &str, cap: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= cap {
        return text;
    }
    let head: String = text.chars().take(cap.saturating_sub(1)).collect();
    format!("{head}…")
}

pub fn compaction_fallback(data: &Value) -> Option<Value> {
    let result = data.get("result")?;
    if result.get("type")?.as_str()? != "compacted" {
        return None;
    }
    Some(
        json!({"type":"compaction", "tokensBefore":result.get("before_tokens")?.as_u64()?,
        "summary":compaction_text(result, data.get("reason").and_then(Value::as_str).unwrap_or("threshold"))}),
    )
}

pub fn compaction_text(result: &Value, reason: &str) -> String {
    let method = match result.get("method").and_then(Value::as_str) {
        Some("remote") => "remote",
        Some("remote_failed_local") => "remote failed → local",
        _ => "local",
    };
    let mut text = format!("{method} · {reason}");
    for (before, after, unit) in [
        ("before_tokens", "after_tokens", "tokens"),
        ("before_message_count", "after_message_count", "messages"),
    ] {
        if let (Some(before), Some(after)) = (
            result.get(before).and_then(Value::as_u64),
            result.get(after).and_then(Value::as_u64),
        ) {
            text.push_str(&format!(" · {before} → {after} {unit}"));
        }
    }
    if let Some(bytes) = result.get("remote_blob_bytes").and_then(Value::as_u64) {
        text.push_str(&format!(" · blob {bytes} B"));
    }
    if let Some(reason) = result
        .get("fallback_reason")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        text.push_str(&format!("\nFallback: {reason}"));
    }
    text
}
