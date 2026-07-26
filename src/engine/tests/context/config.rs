//! Tests for compaction budget derivation — how a model's context window and
//! user settings become trigger/retention budgets.

use evotengine::context::CompactionConfig;
use evotengine::context::ContextConfig;
use evotengine::context::DEFAULT_KEEP_RECENT_TOKENS;
use evotengine::context::DEFAULT_RESERVE_TOKENS;
use evotengine::context::DEFAULT_SUMMARY_MAX_BYTES;
use evotengine::context::SUMMARIZER_INPUT_MAX_BYTES;

/// Reserve and retention budgets are fixed token counts, not a share of the
/// window (pi's `DEFAULT_COMPACTION_SETTINGS`). Scaling them with the window
/// over-reserves on large-context models: a 272k window would otherwise hold
/// back 34k for output and pin a 54k tail.
#[test]
fn budgets_are_fixed_regardless_of_window_size() {
    for window in [128_000, 272_000, 1_048_576] {
        let cfg = CompactionConfig::from_context_window(window);
        assert_eq!(cfg.reserve_tokens, DEFAULT_RESERVE_TOKENS, "{window}");
        assert_eq!(
            cfg.keep_recent_tokens, DEFAULT_KEEP_RECENT_TOKENS,
            "{window}"
        );
        assert_eq!(cfg.trigger_threshold(), window - DEFAULT_RESERVE_TOKENS);
        assert_eq!(cfg.summary_max_bytes, DEFAULT_SUMMARY_MAX_BYTES);
        assert_eq!(
            cfg.summarizer_input_max_bytes(),
            (window - DEFAULT_RESERVE_TOKENS)
                .saturating_mul(4)
                .min(SUMMARIZER_INPUT_MAX_BYTES)
        );
    }
}

#[test]
fn context_config_defaults_leave_budgets_unset() {
    let cfg = CompactionConfig::from_context_config(&ContextConfig::from_context_window(272_000));
    assert_eq!(cfg.context_window, 272_000);
    assert_eq!(cfg.reserve_tokens, DEFAULT_RESERVE_TOKENS);
    assert_eq!(cfg.keep_recent_tokens, DEFAULT_KEEP_RECENT_TOKENS);
}

/// Explicit settings win, so a caller can shrink budgets to fit a small window.
#[test]
fn explicit_budgets_override_the_defaults() {
    let cfg = CompactionConfig::from_context_config(&ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        reserve_tokens: Some(125),
        keep_recent_tokens: Some(200),
    });
    assert_eq!(cfg.reserve_tokens, 125);
    assert_eq!(cfg.keep_recent_tokens, 200);
    assert_eq!(cfg.trigger_threshold(), 875);
    assert_eq!(cfg.summarizer_input_max_bytes(), 3_500);
}

/// A window smaller than the default reserve must not wrap around into a huge
/// threshold; saturating arithmetic floors it at zero (always compact).
#[test]
fn threshold_saturates_when_window_is_below_the_reserve() {
    let cfg = CompactionConfig::from_context_window(8_000);
    assert_eq!(cfg.trigger_threshold(), 0);
    assert_eq!(cfg.summarizer_input_max_bytes(), 0);
}

#[test]
fn unknown_window_uses_the_transport_ceiling() {
    let cfg = CompactionConfig::from_context_window(0);
    assert_eq!(cfg.summarizer_input_max_bytes(), SUMMARIZER_INPUT_MAX_BYTES);
}
