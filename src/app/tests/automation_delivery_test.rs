use std::time::Duration;

use evot::automation::delivery::deliver;
use evot::automation::delivery::NOT_REQUESTED;
use evot::automation::dispatcher::effective_timeout;
use evot::automation::dispatcher::MAX_TIMEOUT_SECONDS;
use evot::automation::dispatcher::MIN_TIMEOUT_SECONDS;
use evot::automation::ExecutorCapabilities;
use evot::conf::channels::FeishuChannelConfig;
use evot::conf::ChannelsConfig;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn channels(default_chat_id: Option<&str>) -> ChannelsConfig {
    ChannelsConfig {
        feishu: default_chat_id.map(|chat| FeishuChannelConfig {
            app_id: "cli_app".into(),
            app_secret: "app_secret".into(),
            mention_only: false,
            allow_from: vec![],
            default_chat_id: chat.into(),
        }),
    }
}

/// A task with no delivery channel must never touch the network.
#[tokio::test]
async fn no_channel_reports_not_requested() -> TestResult {
    assert_eq!(
        deliver(&channels(None), "", "", "result").await?,
        NOT_REQUESTED
    );
    assert_eq!(
        deliver(&channels(Some("oc_default")), "  ", "oc_x", "result").await?,
        NOT_REQUESTED
    );
    Ok(())
}

#[tokio::test]
async fn unknown_channel_is_rejected_before_sending() {
    let error = match deliver(&channels(None), "telegram", "chat", "result").await {
        Ok(status) => panic!("expected an error, got {status}"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("unsupported delivery channel"), "{error}");
}

#[tokio::test]
async fn feishu_delivery_without_the_channel_configured_is_rejected() {
    let error = match deliver(&channels(None), "feishu", "oc_chat", "result").await {
        Ok(status) => panic!("expected an error, got {status}"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("not configured"), "{error}");
}

/// Target resolution happens before any request, so a task that asks for
/// delivery with no reachable chat fails fast rather than half-sending.
#[tokio::test]
async fn feishu_delivery_without_a_target_is_rejected_before_sending() {
    let error = match deliver(&channels(Some("")), "feishu", "", "result").await {
        Ok(status) => panic!("expected an error, got {status}"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("default notification chat ID"), "{error}");
}

#[test]
fn run_timeouts_stay_inside_the_advertised_range() {
    assert_eq!(effective_timeout(900), Duration::from_secs(900));
    assert_eq!(
        effective_timeout(1),
        Duration::from_secs(MIN_TIMEOUT_SECONDS as u64)
    );
    assert_eq!(
        effective_timeout(7_200),
        Duration::from_secs(MAX_TIMEOUT_SECONDS as u64)
    );
    assert_eq!(
        effective_timeout(0),
        Duration::from_secs(MIN_TIMEOUT_SECONDS as u64)
    );
}

/// Capability changes must produce a new fingerprint, or the dispatcher would
/// keep advertising a stale channel set after the console is edited.
#[test]
fn capability_fingerprint_tracks_channel_changes() {
    let linked = ExecutorCapabilities::from_channels(&channels(Some("oc_default")));
    let unlinked = ExecutorCapabilities::from_channels(&channels(None));
    assert!(linked.feishu_ready);
    assert!(!unlinked.feishu_ready);
    assert_ne!(
        linked.fingerprint("user", "exec_1", "host"),
        unlinked.fingerprint("user", "exec_1", "host")
    );
    assert_eq!(
        linked.fingerprint("user", "exec_1", "host"),
        ExecutorCapabilities::from_channels(&channels(Some("oc_other")))
            .fingerprint("user", "exec_1", "host")
    );
    assert_ne!(
        linked.fingerprint("user", "exec_1", "host"),
        linked.fingerprint("user", "exec_2", "host")
    );
}
