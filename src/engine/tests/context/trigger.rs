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
        total_tokens: 0,
        model: model_id(),
        timestamp: 1000,
        stop_reason: stop,
        error_message: None,
    }
}

#[test]
fn unknown_window_only_recovers_explicit_overflow() {
    let mut config = default_config();
    config.context_window = 0;

    // A successful response must not read as silent overflow or threshold
    // when the window is unknown; pi gates both behind `contextWindow &&`.
    let stop = TriggerInput {
        usage: Some(make_usage(500_000, 1_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&stop, &config), TriggerDecision::Skip);

    let length = TriggerInput {
        usage: Some(make_usage(500_000, 0, StopReason::Length)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&length, &config), TriggerDecision::Skip);

    // Explicit overflow errors carry their own signal and still recover.
    let mut overflow = make_usage(0, 0, StopReason::Error);
    overflow.error_message = Some("prompt is too long".into());
    let error = TriggerInput {
        usage: Some(overflow),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&error, &config), TriggerDecision::Overflow {
        context_tokens: 0,
        will_retry: true,
    });
}

#[test]
fn native_total_takes_precedence() {
    let config = default_config();
    let mut usage = make_usage(1_000, 100, StopReason::Stop);
    usage.total_tokens = config.trigger_threshold() + 1;
    let input = TriggerInput {
        usage: Some(usage),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };

    assert_eq!(evaluate(&input, &config), TriggerDecision::Threshold {
        context_tokens: config.trigger_threshold() + 1
    });
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
fn aborted_usage_can_trigger_before_prompt_threshold() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(120_000, 1_000, StopReason::Aborted)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&input, &config), TriggerDecision::Threshold {
        context_tokens: 121_000
    });
}

#[test]
fn model_mismatch_skips_all_automatic_compaction_signals() {
    let config = default_config();
    let current_model = ModelId {
        provider: "openai".into(),
        model: "gpt-4".into(),
    };

    // A previous model being near its own boundary says nothing about the newly
    // selected model's boundary. The first request goes to the selected model
    // unchanged, matching Droid's response-driven policy.
    let successful = TriggerInput {
        usage: Some(make_usage(120_000, 1_000, StopReason::Stop)),
        current_model: current_model.clone(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&successful, &config), TriggerDecision::Skip);

    // An overflow from the old model must not compact for the new model.
    let mut overflow = make_usage(0, 0, StopReason::Error);
    overflow.error_message = Some("Context overflow: request too large".into());
    let stale_overflow = TriggerInput {
        usage: Some(overflow),
        current_model,
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    assert_eq!(evaluate(&stale_overflow, &config), TriggerDecision::Skip);
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
fn overflow_exhausted_when_already_attempted() {
    let config = default_config();
    let mut usage = make_usage(0, 0, StopReason::Error);
    usage.error_message = Some("prompt is too long: 200000 tokens > 128000 maximum".into());
    let input = TriggerInput {
        usage: Some(usage),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: true,
    };
    assert!(matches!(
        evaluate(&input, &config),
        TriggerDecision::OverflowExhausted { .. }
    ));
}

#[test]
fn successful_silent_overflow_remains_accepted_after_retry() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(130_000, 1_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: true,
    };

    assert_eq!(evaluate(&input, &config), TriggerDecision::Overflow {
        context_tokens: 131_000,
        will_retry: false,
    });
}

#[test]
fn successful_silent_overflow_compacts_without_retry() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(130_000, 1_000, StopReason::Stop)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };

    assert_eq!(evaluate(&input, &config), TriggerDecision::Overflow {
        context_tokens: 131_000,
        will_retry: false,
    });
}

#[test]
fn length_stop_with_partial_output_over_window_is_threshold() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(127_900, 200, StopReason::Length)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };

    assert_eq!(evaluate(&input, &config), TriggerDecision::Threshold {
        context_tokens: 128_100
    });
}

#[test]
fn length_stop_with_zero_output_at_window_is_retryable_overflow() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(127_000, 0, StopReason::Length)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };

    assert_eq!(evaluate(&input, &config), TriggerDecision::Overflow {
        context_tokens: 127_000,
        will_retry: true,
    });
}

#[test]
fn length_stop_with_partial_output_below_window_is_threshold_not_overflow() {
    let config = default_config();
    let input = TriggerInput {
        usage: Some(make_usage(112_000, 200, StopReason::Length)),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };

    assert_eq!(evaluate(&input, &config), TriggerDecision::Threshold {
        context_tokens: 112_200
    });
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

#[test]
fn overflow_with_try_again_wording_still_triggers() {
    // Regression: an overflow error whose text also contains "try again" must
    // route to Overflow (compact-and-retry), not be skipped. The trigger shares
    // the provider error classifier, so this stays consistent with retry logic.
    let config = default_config();
    let mut usage = make_usage(0, 0, StopReason::Error);
    usage.error_message = Some(
        "Your input exceeds the context window of this model. \
         Please adjust your input and try again."
            .into(),
    );
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
fn throttling_error_is_not_overflow() {
    // "too many tokens" throttling wording must NOT be treated as overflow.
    let config = default_config();
    let mut usage = make_usage(0, 0, StopReason::Error);
    usage.error_message =
        Some("ThrottlingException: Too many tokens, please wait before trying again.".into());
    let input = TriggerInput {
        usage: Some(usage),
        current_model: model_id(),
        last_compaction_ts: None,
        overflow_recovery_attempted: false,
    };
    // Error stop reason that is not overflow -> Skip (no usable usage data).
    assert_eq!(evaluate(&input, &config), TriggerDecision::Skip);
}
