//! Generic, provider-agnostic model config resolution in `build_model_config`.
//!
//! Verifies the explicit override behavior: `context_window`, `max_tokens`, and
//! `supports_image` are applied when provided, and left at the protocol default
//! otherwise. These are set via `EVOT_LLM_<PROVIDER>_*` for any provider.

use evot::agent::run::runtime::build_model_config;
use evot::conf::Protocol;
use evot_engine::provider::CompatCaps;
use evot_engine::provider::InputModality;

#[test]
fn openai_compatible_defaults_to_text_only() {
    let mc = build_model_config(
        Protocol::OpenAi,
        "openrouter",
        "some/model",
        Some("https://openrouter.ai/api/v1"),
        CompatCaps::NONE,
        None,
        None,
        None,
    );
    // Unknown OpenAI-compatible models are conservatively text-only.
    assert!(!mc.supports_image());
    assert_eq!(mc.input, vec![InputModality::Text]);
    // Conservative default window when none configured.
    assert_eq!(mc.context_window, 128_000);
}

#[test]
fn explicit_supports_image_enables_vision() {
    let mc = build_model_config(
        Protocol::OpenAi,
        "openrouter",
        "some/vision-model",
        Some("https://openrouter.ai/api/v1"),
        CompatCaps::NONE,
        None,
        None,
        Some(true),
    );
    assert!(mc.supports_image());
    assert_eq!(mc.input, vec![InputModality::Text, InputModality::Image]);
}

#[test]
fn explicit_supports_image_false_forces_text_only() {
    // Even a protocol that defaults to vision (Anthropic) can be pinned to
    // text-only when the configured endpoint does not accept images.
    let mc = build_model_config(
        Protocol::Anthropic,
        "anthropic",
        "claude-sonnet-4",
        None,
        CompatCaps::NONE,
        None,
        None,
        Some(false),
    );
    assert!(!mc.supports_image());
    assert_eq!(mc.input, vec![InputModality::Text]);
}

#[test]
fn explicit_context_window_and_max_tokens_apply() {
    let mc = build_model_config(
        Protocol::OpenAi,
        "openrouter",
        "tencent/hy3:free",
        Some("https://openrouter.ai/api/v1"),
        CompatCaps::NONE,
        Some(262_144),
        Some(8_192),
        None,
    );
    assert_eq!(mc.context_window, 262_144);
    assert_eq!(mc.max_tokens, 8_192);
}

#[test]
fn native_openai_gpt_5_6_metadata_applies_without_explicit_overrides() {
    let mc = build_model_config(
        Protocol::OpenAi,
        "openai",
        "gpt-5.6-sol",
        Some("https://api.openai.com/v1"),
        CompatCaps::NONE,
        None,
        None,
        None,
    );
    assert_eq!(mc.context_window, 272_000);
    assert_eq!(mc.max_tokens, 128_000);
    assert!(mc.supports_image());
}

#[test]
fn openrouter_gpt_5_6_gets_catalog_limits_without_explicit_overrides() {
    let mc = build_model_config(
        Protocol::OpenAi,
        "openrouter",
        "openai/gpt-5.6-sol",
        Some("https://openrouter.ai/api/v1"),
        CompatCaps::REASONING_EFFORT,
        None,
        None,
        None,
    );
    assert_eq!(mc.context_window, 272_000);
    assert_eq!(mc.max_tokens, 128_000);
    assert_eq!(
        mc.thinking_effort_override(evot_engine::ThinkingLevel::Max),
        Some("max")
    );
}

#[test]
fn anthropic_defaults_to_vision() {
    let mc = build_model_config(
        Protocol::Anthropic,
        "anthropic",
        "claude-sonnet-4",
        None,
        CompatCaps::NONE,
        None,
        None,
        None,
    );
    assert!(mc.supports_image());
}
