//! Runtime context — injected before each turn so the LLM knows "when" and "where".

use std::fmt::Write;

use chrono::Local;

/// Build a runtime context block with current time, timezone, OS, and optional channel info.
pub fn build_runtime_context(channel_type: Option<&str>, chat_id: Option<&str>) -> String {
    let mut buf = String::with_capacity(256);
    buf.push_str("## Runtime\n\n");

    // Current time + timezone
    let now = Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M (%A)").to_string();
    let tz = now.format("%Z").to_string();
    let _ = writeln!(buf, "Current Time: {time_str} ({tz})");

    // OS / arch
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let _ = writeln!(buf, "Platform: {os} ({arch})");

    // Channel info (if running from a channel like feishu/telegram)
    if let Some(ch) = channel_type.filter(|s| !s.is_empty()) {
        let _ = write!(buf, "Channel: {ch}");
        if let Some(cid) = chat_id.filter(|s| !s.is_empty()) {
            let _ = write!(buf, " (chat: {cid})");
        }
        buf.push('\n');
    }

    buf.push('\n');
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_context_includes_time_and_platform() {
        let ctx = build_runtime_context(None, None);
        assert!(ctx.contains("Current Time:"));
        assert!(ctx.contains("Platform:"));
        assert!(!ctx.contains("Channel:"));
    }

    #[test]
    fn runtime_context_includes_channel_when_provided() {
        let ctx = build_runtime_context(Some("feishu"), Some("oc_abc123"));
        assert!(ctx.contains("Channel: feishu (chat: oc_abc123)"));
    }

    #[test]
    fn runtime_context_skips_empty_channel() {
        let ctx = build_runtime_context(Some(""), None);
        assert!(!ctx.contains("Channel:"));
    }
}
