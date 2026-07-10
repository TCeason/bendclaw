use evotengine::provider::model::*;

#[test]
fn model_config_anthropic() {
    let config = ModelConfig::anthropic("claude-sonnet-4-20250514", "Claude Sonnet 4");
    assert_eq!(config.api, ApiProtocol::AnthropicMessages);
    assert_eq!(config.provider, "anthropic");
    assert!(config.compat.is_none());
}

#[test]
fn model_config_openai() {
    let config = ModelConfig::openai("gpt-4o", "GPT-4o");
    assert_eq!(config.api, ApiProtocol::OpenAiCompletions);
    assert_eq!(config.context_window, 128_000);
    let compat = config.compat.unwrap();
    assert!(compat.caps.contains(CompatCaps::STORE));
    assert!(compat.caps.contains(CompatCaps::DEVELOPER_ROLE));
    assert_eq!(compat.max_tokens_field, MaxTokensField::MaxCompletionTokens);
}

#[test]
fn model_config_openai_current_gpt_models_use_catalog_limits() {
    for id in [
        "gpt-5.4",
        "gpt-5.5",
        "gpt-5.6-luna",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
    ] {
        let config = ModelConfig::openai(id, id);
        assert_eq!(config.context_window, 272_000, "{id}");
        assert_eq!(config.max_tokens, 128_000, "{id}");

        let local_config = ModelConfig::local("", id);
        assert_eq!(local_config.context_window, 272_000, "{id}");
        assert_eq!(local_config.max_tokens, 128_000, "{id}");
    }

    for id in ["gpt-5.4-pro", "gpt-5.5-pro"] {
        let config = ModelConfig::openai(id, id);
        assert_eq!(config.context_window, 1_050_000, "{id}");
        assert_eq!(config.max_tokens, 128_000, "{id}");
    }
}

#[test]
fn model_config_openai_gpt_reasoning_level_map_matches_codex_defaults() {
    use evotengine::ThinkingLevel;

    let gpt_5_5 = ModelConfig::openai("gpt-5.5", "GPT-5.5");
    assert_eq!(
        gpt_5_5.thinking_effort_override(ThinkingLevel::Adaptive),
        Some("medium")
    );
    assert_eq!(
        gpt_5_5.thinking_effort_override(ThinkingLevel::Xhigh),
        Some("xhigh")
    );

    let gpt_5_4 = ModelConfig::openai("gpt-5.4", "GPT-5.4");
    assert_eq!(
        gpt_5_4.thinking_effort_override(ThinkingLevel::Adaptive),
        Some("xhigh")
    );

    let runtime_path = ModelConfig::local("", "gpt-5.5");
    assert_eq!(
        runtime_path.thinking_effort_override(ThinkingLevel::Adaptive),
        Some("medium")
    );
}

#[test]
fn model_config_anthropic_opus_4_6_4_7_4_8_use_1m_context() {
    for id in [
        "claude-opus-4-6",
        "claude-opus-4.6",
        "claude-opus-4-7",
        "claude-opus-4.7",
        "claude-opus-4-8",
        "claude-opus-4.8",
    ] {
        let config = ModelConfig::anthropic(id, "Opus");
        assert_eq!(config.context_window, 1_000_000, "{id}");
        assert_eq!(config.max_tokens, 128_000, "{id}");
    }

    // Opus 4.5 predates the 1M window but is still modern 4.x: 200k window,
    // 64k output budget.
    let older = ModelConfig::anthropic("claude-opus-4.5", "Opus 4.5");
    assert_eq!(older.context_window, 200_000);
    assert_eq!(older.max_tokens, 64_000);

    // Real ids carry a date suffix; version parsing must still apply the gate.
    let dated = ModelConfig::anthropic("claude-opus-4-6-20251101", "Opus 4.6");
    assert_eq!(dated.context_window, 1_000_000);
    assert_eq!(dated.max_tokens, 128_000);

    // A future Opus is covered by the >= 4.6 version gate, no id list edit.
    let future = ModelConfig::anthropic("claude-opus-5-0", "Opus 5");
    assert_eq!(future.context_window, 1_000_000);
}

#[test]
fn openai_compat_variants() {
    let xai = OpenAiCompat::xai();
    assert_eq!(xai.thinking_format, ThinkingFormat::Xai);
    assert!(!xai.caps.contains(CompatCaps::STORE));

    let groq = OpenAiCompat::groq();
    assert!(groq.caps.contains(CompatCaps::USAGE_IN_STREAMING));
    assert!(!groq.caps.contains(CompatCaps::STORE));

    let deepseek = OpenAiCompat::deepseek();
    assert_eq!(
        deepseek.max_tokens_field,
        MaxTokensField::MaxCompletionTokens
    );

    let zai = OpenAiCompat::zai();
    assert!(zai.caps.contains(CompatCaps::USAGE_IN_STREAMING));
    assert!(!zai.caps.contains(CompatCaps::STORE));

    let minimax = OpenAiCompat::minimax();
    assert!(minimax.caps.contains(CompatCaps::USAGE_IN_STREAMING));
    assert!(!minimax.caps.contains(CompatCaps::STORE));
}

#[test]
fn model_config_local_defaults_to_generous_output_cap() {
    // Every OpenAI-protocol provider is built through `local()` at runtime, so
    // its default output cap must be generous (the request builder clamps it to
    // the remaining context window per call). A small cap here would truncate
    // long responses regardless of the window.
    let config = ModelConfig::local("https://api.example.com/v1", "some-model");
    assert_eq!(config.max_tokens, 32_768);
    assert_eq!(config.provider, "local");
}

#[test]
fn model_config_anthropic_modern_models_use_64k_output_cap() {
    // Modern Claude 4.x (non-1M) support a 64k output budget. The old 8192
    // default silently truncated long responses.
    for id in [
        "claude-sonnet-4-20250514",
        "claude-sonnet-4-5",
        "claude-haiku-4-5",
        "claude-opus-4-1",
    ] {
        let config = ModelConfig::anthropic(id, "Claude");
        assert_eq!(config.context_window, 200_000, "{id}");
        assert_eq!(config.max_tokens, 64_000, "{id}");
    }

    // Sonnet 4.6 and Sonnet 5+ carry the 1M window and the 128k output cap.
    for id in ["claude-sonnet-4-6", "claude-sonnet-5"] {
        let sonnet = ModelConfig::anthropic(id, "Sonnet");
        assert_eq!(sonnet.context_window, 1_000_000, "{id}");
        assert_eq!(sonnet.max_tokens, 128_000, "{id}");
    }

    // Legacy claude-3 keeps the conservative fallback.
    let legacy = ModelConfig::anthropic("claude-3-opus-20240229", "Opus 3");
    assert_eq!(legacy.context_window, 200_000);
    assert_eq!(legacy.max_tokens, 8192);
}

#[test]
fn model_config_anthropic_fable_5_uses_1m_context_and_cannot_disable_thinking() {
    // Fable is a distinct family (no opus/sonnet/haiku version gate): 1M window,
    // 128k output cap, and `off` thinking is unsupported. Mirrors pi's
    // claude-fable-5 registry entry (thinkingLevelMap {off:null}).
    let config = ModelConfig::anthropic("claude-fable-5", "Claude Fable 5");
    assert_eq!(config.context_window, 1_000_000);
    assert_eq!(config.max_tokens, 128_000);
    assert!(!config.can_disable_thinking());
}

#[test]
fn api_protocol_display() {
    assert_eq!(
        ApiProtocol::AnthropicMessages.to_string(),
        "anthropic_messages"
    );
    assert_eq!(
        ApiProtocol::OpenAiCompletions.to_string(),
        "openai_completions"
    );
    assert_eq!(
        ApiProtocol::BedrockConverseStream.to_string(),
        "bedrock_converse_stream"
    );
}

#[test]
fn cost_config_default() {
    let cost = CostConfig::default();
    assert_eq!(cost.input_per_million, 0.0);
    assert_eq!(cost.output_per_million, 0.0);
}

#[test]
fn supported_thinking_levels_anthropic_use_model_specific_extended_tiers() {
    use evotengine::ThinkingLevel::*;

    let opus_4_6 = ModelConfig::anthropic("claude-opus-4-6", "Opus 4.6");
    assert_eq!(opus_4_6.supported_thinking_levels(), vec![
        Off, Low, Medium, High, Max
    ]);

    let opus_4_8 = ModelConfig::anthropic("claude-opus-4-8", "Opus 4.8");
    assert_eq!(opus_4_8.supported_thinking_levels(), vec![
        Off, Low, Medium, High, Xhigh, Max
    ]);
}

#[test]
fn supported_thinking_levels_openai_with_effort_includes_xhigh_when_mapped() {
    use evotengine::ThinkingLevel::*;
    // GPT-5 family advertises reasoning effort and maps xhigh explicitly.
    let config = ModelConfig::openai("gpt-5.5", "GPT-5.5");
    assert!(config
        .compat
        .as_ref()
        .unwrap()
        .caps
        .contains(CompatCaps::REASONING_EFFORT));
    assert_eq!(config.supported_thinking_levels(), vec![
        Off, Low, Medium, High, Xhigh
    ]);
}

#[test]
fn supported_thinking_levels_openai_without_xhigh_map_stops_at_high() {
    use evotengine::ThinkingLevel::*;
    // A bare OpenAI-completions model with reasoning effort but no xhigh map
    // entry should not offer xhigh (it would collapse onto high).
    let mut config = ModelConfig::local("", "some-reasoner");
    config.thinking_level_map.clear();
    if let Some(compat) = &mut config.compat {
        compat.caps |= CompatCaps::REASONING_EFFORT;
    }
    assert_eq!(config.supported_thinking_levels(), vec![
        Off, Low, Medium, High
    ]);
}

#[test]
fn supported_thinking_levels_openai_without_effort_capability_is_empty() {
    // deepseek's OpenAI-compat profile lacks REASONING_EFFORT, so the reasoning
    // effort field is inert and no levels should be selectable.
    let mut config = ModelConfig::local("", "deepseek-chat");
    config.compat = Some(OpenAiCompat::deepseek());
    assert!(!config
        .compat
        .as_ref()
        .unwrap()
        .caps
        .contains(CompatCaps::REASONING_EFFORT));
    assert!(config.supported_thinking_levels().is_empty());
}

#[test]
fn supported_thinking_levels_gpt_5_5_pro_drops_off_minimal_low() {
    use evotengine::ThinkingLevel::*;
    // gpt-5.5-pro maps off/minimal/low to None (unsupported); medium is its
    // floor. Matches pi's per-model thinkingLevelMap exactly.
    let config = ModelConfig::openai("gpt-5.5-pro", "GPT-5.5 Pro");
    assert_eq!(config.supported_thinking_levels(), vec![
        Medium, High, Xhigh
    ]);
}

#[test]
fn supported_thinking_levels_gpt_5_5_drops_minimal_only() {
    use evotengine::ThinkingLevel::*;
    // gpt-5.5 (non-pro) drops only `minimal`; minimal is never in the ramp
    // anyway, so the full off..xhigh cycle remains.
    let config = ModelConfig::openai("gpt-5.5", "GPT-5.5");
    assert_eq!(config.supported_thinking_levels(), vec![
        Off, Low, Medium, High, Xhigh
    ]);
}

#[test]
fn supported_thinking_levels_gpt_5_6_includes_xhigh_and_max() {
    use evotengine::ThinkingLevel::*;

    for id in ["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"] {
        let config = ModelConfig::openai(id, id);
        assert_eq!(
            config.supported_thinking_levels(),
            vec![Off, Low, Medium, High, Xhigh, Max],
            "{id}"
        );
        assert_eq!(config.thinking_effort_override(Max), Some("max"), "{id}");
    }
}

#[test]
fn openrouter_prefixed_gpt_5_6_gets_catalog_limits_and_reasoning_metadata() {
    use evotengine::ThinkingLevel::*;

    let mut config = ModelConfig::local("", "openai/gpt-5.6-sol");
    config.compat = Some(OpenAiCompat::openai());
    assert_eq!(config.context_window, 272_000);
    assert_eq!(config.max_tokens, 128_000);
    assert_eq!(config.supported_thinking_levels(), vec![
        Off, Low, Medium, High, Xhigh, Max
    ]);
}

#[test]
fn thinking_effort_override_distinguishes_unsupported_from_default() {
    use evotengine::ThinkingLevel;
    // gpt-5.5-pro: `low` is explicitly unsupported (None), `xhigh` maps to a
    // concrete effort, and `high` has no entry (protocol default).
    let config = ModelConfig::openai("gpt-5.5-pro", "GPT-5.5 Pro");
    assert_eq!(config.thinking_effort_override(ThinkingLevel::Low), None);
    assert_eq!(
        config.thinking_effort_override(ThinkingLevel::Xhigh),
        Some("xhigh")
    );
    assert_eq!(config.thinking_effort_override(ThinkingLevel::High), None);
}
