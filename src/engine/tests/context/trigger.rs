//! Tests for the trigger module.

use evotengine::context::compaction::config::CompactionConfig;
use evotengine::context::compaction::trigger::evaluate;
use evotengine::context::compaction::trigger::TriggerInput;
use evotengine::context::compaction::types::ModelId;
use evotengine::context::compaction::types::TriggerDecision;
use evotengine::context::compaction::types::UsageSnapshot;
use evotengine::types::StopReason;

fn default_config() -> CompactionConfig {
    CompactionConfig::from_context_window(128_000)
}

fn model_id() -> ModelId {
    ModelId {
        provider: "anthropic".into(),
        model: "claude-3".into(),
    }
}

fn make_usage(input: usize, output: usize, stop: StopReason) -> UsageSnapshot {
    UsageSnapshot {
        input,
        cache_read: 0,
        cache_write: 0,
        output,
        model: model_id(),
        timestamp: 1000,
        stop_reason: stop,
        error_message: None,
    }
}

#[test]
fn skip_when_no_usage() {
    let config = default_config();
    let input = TriggerInput {
        usage: None,
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}

#[test]
fn skip_when_aborted() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(50_000, 1_000, StopReason::Aborted)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}

#[test]
fn skip_when_model_mismatch() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(120_000, 1_000, StopReason::Stop)),
        current_model: ModelId {
            provider: "openai".into(),
            model: "gpt-4".into(),
        },
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}

#[test]
fn skip_when_stale_usage() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(120_000, 1_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: Some(2000), // compaction happened after usage timestamp
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}

#[test]
fn threshold_when_over_limit() {
    let config = default_config();
    // trigger_threshold = 128_000 - 16_384 = 111_616
    let input = TriggerInput {
        usage: Some(make_usage(110_000, 5_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    // context_tokens = 110_000 + 5_000 = 115_000 > 111_616
    assert_eq!(evaluate(&input, &config), TriggerDecision::Threshold {
        context_tokens: 115_000
    });
}

#[test]
fn overflow_on_error_message() {
    let config = default_config();
    let mut usage = make_usage(0, 0, StopReason::Error);
    usage.error_message = Some("prompt is too long: 200000 tokens > 128000 maximum".into());
    let input = TriggerInput {
        usage: Some(usage),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert!(matches!(
        evaluate(&input, &config),
        TriggerDecision::Overflow { .. }
    ));
}

#[test]
fn skip_overflow_when_already_attempted() {
    let config = default_config();
    let mut usage = make_usage(0, 0, StopReason::Error);
    usage.error_message = Some("prompt is too long: 200000 tokens > 128000 maximum".into());
    let input = TriggerInput {
        usage: Some(usage),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: true,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}

#[test]
fn silent_overflow_detected() {
    let config = default_config();
    // input exceeds context_window
    let input = TriggerInput {
        usage: Some(make_usage(130_000, 1_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert!(matches!(
        evaluate(&input, &config),
        TriggerDecision::Overflow { .. }
    ));
}

#[test]
fn below_threshold_is_skip() {
    let config = default_config();
    // context_tokens = 50_000 + 1_000 = 51_000 < 111_616
    let input = TriggerInput {
        usage: Some(make_usage(50_000, 1_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}
