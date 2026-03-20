use bendclaw::kernel::run::runtime_context::build_runtime_context;

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
