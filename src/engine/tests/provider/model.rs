use std::collections::HashMap;

use evotengine::provider::*;
use evotengine::ThinkingLevel;

fn resolved(
    protocol: ApiProtocol,
    provider: &str,
    model_id: &str,
    base_url: &str,
    compat: Option<OpenAiCompat>,
    route_capabilities: RouteCapabilities,
    overrides: ModelOverrides,
) -> ModelConfig {
    ModelConfig::resolve(ResolveModelRequest {
        protocol,
        provider: provider.into(),
        model_id: model_id.into(),
        base_url: base_url.into(),
        headers: HashMap::new(),
        compat,
        route_capabilities,
        overrides,
    })
}

#[test]
fn model_config_presets_resolve_expected_routes() {
    let anthropic = ModelConfig::anthropic("claude-sonnet-4-20250514", "Claude Sonnet 4");
    assert_eq!(anthropic.protocol(), ApiProtocol::AnthropicMessages);
    assert_eq!(anthropic.provider(), "anthropic");
    assert!(anthropic.compat().is_none());

    let openai = ModelConfig::openai("gpt-4o", "GPT-4o");
    assert_eq!(openai.protocol(), ApiProtocol::OpenAiCompletions);
    assert_eq!(openai.context_window(), 128_000);
    let Some(compat) = openai.compat() else {
        panic!("OpenAI preset must carry compatibility metadata");
    };
    assert!(compat.caps.contains(CompatCaps::STORE));
    assert!(compat.caps.contains(CompatCaps::DEVELOPER_ROLE));
    assert_eq!(compat.max_tokens_field, MaxTokensField::MaxCompletionTokens);

    let responses = ModelConfig::openai_responses("gpt-5.5", "GPT-5.5");
    assert_eq!(responses.protocol(), ApiProtocol::OpenAiResponses);
    assert_eq!(responses.provider(), "openai");
}

#[test]
fn route_resolution_is_endpoint_aware_and_explicitly_overridable() {
    let official = OpenAiCompat::for_route("openai", "https://api.openai.com/v1/");
    assert!(official.caps.contains(CompatCaps::STORE));
    assert!(official.caps.contains(CompatCaps::DEVELOPER_ROLE));

    let proxy = OpenAiCompat::for_route("openai", "https://proxy.example.com/v1");
    assert!(proxy.caps.contains(CompatCaps::STORE));
    assert!(proxy.caps.contains(CompatCaps::DEVELOPER_ROLE));
    assert!(proxy.caps.contains(CompatCaps::REASONING_EFFORT));

    let official_route = RouteCapabilities::for_route(
        ApiProtocol::OpenAiResponses,
        "openai",
        "https://api.openai.com/v1",
        CompatCaps::NONE,
    );
    assert!(official_route.verbosity);
    assert!(official_route.remote_compaction);

    let databend_route = RouteCapabilities::for_route(
        ApiProtocol::OpenAiResponses,
        "openai",
        "https://openrouter.databend.cloud/openai/v1/",
        CompatCaps::NONE,
    );
    assert!(databend_route.verbosity);
    assert!(databend_route.remote_compaction);

    let proxy_route = RouteCapabilities::for_route(
        ApiProtocol::OpenAiResponses,
        "openai",
        "https://proxy.example.com/v1",
        CompatCaps::VERBOSITY | CompatCaps::REMOTE_COMPACTION,
    );
    assert!(proxy_route.verbosity);
    assert!(proxy_route.remote_compaction);

    let chat_route = RouteCapabilities::for_route(
        ApiProtocol::OpenAiCompletions,
        "openai",
        "https://proxy.example.com/v1",
        CompatCaps::REMOTE_COMPACTION,
    );
    assert!(!chat_route.remote_compaction);
}

#[test]
fn route_and_model_capabilities_are_intersected() {
    let official = ModelConfig::openai_responses("gpt-5.6-sol", "GPT-5.6 Sol");
    assert_eq!(official.effective_verbosity(), Some(Verbosity::Low));
    assert!(official.can_remote_compact());

    let proxy = resolved(
        ApiProtocol::OpenAiResponses,
        "proxy",
        "gpt-5.6-sol",
        "https://proxy.example.com/v1",
        Some(OpenAiCompat::openai()),
        RouteCapabilities::default(),
        ModelOverrides::default(),
    );
    assert_eq!(proxy.effective_verbosity(), None);
    assert!(!proxy.can_remote_compact());

    let verbosity_only = resolved(
        ApiProtocol::OpenAiResponses,
        "proxy",
        "gpt-5.6-sol",
        "https://proxy.example.com/v1",
        Some(OpenAiCompat::openai()),
        RouteCapabilities {
            verbosity: true,
            remote_compaction: false,
        },
        ModelOverrides::default(),
    );
    assert_eq!(verbosity_only.effective_verbosity(), Some(Verbosity::Low));
    assert!(!verbosity_only.can_remote_compact());
}

#[test]
fn remote_compaction_is_allowlisted_by_model_and_route() {
    for id in ["gpt-5.6-sol", "gpt-5.5"] {
        assert!(
            ModelConfig::openai_responses(id, id).can_remote_compact(),
            "{id}"
        );
    }
    for id in [
        "o3",
        "codex-mini",
        "gpt-5.7-nova",
        "grok-4.5",
        "claude-opus-4-6",
        "unknown-model",
    ] {
        assert!(
            !ModelConfig::openai_responses(id, id).can_remote_compact(),
            "{id}"
        );
    }
}

#[test]
fn date_suffixed_anthropic_ids_match_family_capabilities() {
    for (bare, dated) in [
        ("claude-opus-4-6", "claude-opus-4-6-20251101"),
        ("claude-opus-4-8", "anthropic/claude-opus-4-8-20260115"),
        ("claude-sonnet-4-6", "claude-sonnet-4-6-20251201"),
        ("claude-sonnet-5", "claude-sonnet-5-20260101"),
    ] {
        let bare = ModelConfig::anthropic(bare, bare);
        let dated = ModelConfig::anthropic(dated, dated);
        assert_eq!(
            bare.context_window(),
            dated.context_window(),
            "{}",
            dated.id()
        );
        assert_eq!(bare.max_tokens(), dated.max_tokens(), "{}", dated.id());
        assert_eq!(
            bare.supported_thinking_levels(),
            dated.supported_thinking_levels(),
            "{}",
            dated.id()
        );
        assert_eq!(
            bare.can_disable_thinking(),
            dated.can_disable_thinking(),
            "{}",
            dated.id()
        );
    }
}

#[test]
fn kimi_profiles_match_catalog_contracts() {
    for id in ["k2p7", "kimi-for-coding", "kimi-for-coding-highspeed"] {
        let config = ModelConfig::anthropic(id, id);
        assert_eq!(config.context_window(), 262_144, "{id}");
        assert_eq!(config.max_tokens(), 32_768, "{id}");
        assert_eq!(
            config.input(),
            [InputModality::Text, InputModality::Image],
            "{id}"
        );
    }

    use evotengine::ThinkingLevel::*;
    let k3 = ModelConfig::anthropic("k3", "Kimi K3");
    assert_eq!(k3.context_window(), 1_048_576);
    assert_eq!(k3.max_tokens(), 131_072);
    assert_eq!(k3.supported_thinking_levels(), vec![Low, High, Max]);
    assert!(!k3.can_disable_thinking());

    let thinking = ModelConfig::anthropic("kimi-k2-thinking", "Kimi K2 Thinking");
    assert_eq!(thinking.context_window(), 262_144);
    assert_eq!(thinking.max_tokens(), 32_768);
    assert_eq!(thinking.input(), [InputModality::Text]);
}

#[test]
fn current_openai_profiles_expose_limits_and_verbosity() {
    for id in [
        "gpt-5.4",
        "gpt-5.5",
        "gpt-5.6-luna",
        "gpt-5.6-sol",
        "gpt-5.6-terra",
    ] {
        let config = ModelConfig::openai(id, id);
        assert_eq!(config.context_window(), 272_000, "{id}");
        assert_eq!(config.max_tokens(), 128_000, "{id}");
    }
    for id in ["gpt-5.4-pro", "gpt-5.5-pro"] {
        let config = ModelConfig::openai(id, id);
        assert_eq!(config.context_window(), 1_050_000, "{id}");
        assert_eq!(config.max_tokens(), 128_000, "{id}");
    }
    for id in ["gpt-5.5", "gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"] {
        assert_eq!(
            ModelConfig::openai(id, id).effective_verbosity(),
            Some(Verbosity::Low),
            "{id}"
        );
    }
    for id in ["gpt-5.4", "gpt-5.4-pro", "gpt-5.5-pro", "gpt-5.7-nova"] {
        assert_eq!(
            ModelConfig::openai(id, id).effective_verbosity(),
            None,
            "{id}"
        );
    }
}

#[test]
fn unknown_openai_families_keep_reasoning_fallback_without_extensions() {
    use evotengine::ThinkingLevel::*;

    for id in ["codex-mini", "gpt-5.7-nova"] {
        let config = ModelConfig::openai(id, id);
        assert!(config.reasoning(), "{id}");
        assert_eq!(
            config.supported_thinking_levels(),
            vec![Off, Minimal, Low, Medium, High],
            "{id}"
        );
        assert_eq!(config.effective_verbosity(), None, "{id}");
        assert!(!config.can_remote_compact(), "{id}");
    }
}

#[test]
fn anthropic_version_rules_cover_current_and_future_models() {
    for id in ["claude-opus-4-6", "claude-opus-4-8", "claude-opus-5-0"] {
        let config = ModelConfig::anthropic(id, id);
        assert_eq!(config.context_window(), 1_000_000, "{id}");
        assert_eq!(config.max_tokens(), 128_000, "{id}");
    }
    for id in [
        "claude-sonnet-4-20250514",
        "claude-sonnet-4-5",
        "claude-haiku-4-5",
        "claude-opus-4-1",
    ] {
        let config = ModelConfig::anthropic(id, id);
        assert_eq!(config.context_window(), 200_000, "{id}");
        assert_eq!(config.max_tokens(), 64_000, "{id}");
    }
    let legacy = ModelConfig::anthropic("claude-3-opus-20240229", "Opus 3");
    assert_eq!(legacy.context_window(), 200_000);
    assert_eq!(legacy.max_tokens(), 8_192);
}

#[test]
fn explicit_overrides_apply_after_catalog_resolution() {
    let config = resolved(
        ApiProtocol::OpenAiCompletions,
        "custom",
        "gpt-5.6-sol",
        "https://example.com/v1",
        Some(OpenAiCompat::default()),
        RouteCapabilities::default(),
        ModelOverrides {
            context_window: Some(64_000),
            max_output_tokens: Some(4_096),
            supports_image: Some(false),
            reasoning: Some(false),
        },
    );
    assert_eq!(config.context_window(), 64_000);
    assert_eq!(config.max_tokens(), 4_096);
    assert!(!config.supports_image());
    assert!(!config.reasoning());
    assert_eq!(config.supported_thinking_levels(), vec![ThinkingLevel::Off]);
}

#[test]
fn openai_compat_profiles_are_transport_only() {
    let xai = OpenAiCompat::xai();
    assert_eq!(xai.thinking_format, ThinkingFormat::Xai);
    assert!(!xai.caps.contains(CompatCaps::STORE));
    assert!(!xai.caps.contains(CompatCaps::REASONING_EFFORT));

    let grok_cli = OpenAiCompat::grok_cli();
    assert!(grok_cli.caps.contains(CompatCaps::REASONING_EFFORT));

    let deepseek = OpenAiCompat::deepseek();
    assert_eq!(
        deepseek.max_tokens_field,
        MaxTokensField::MaxCompletionTokens
    );
    assert_eq!(deepseek.thinking_format, ThinkingFormat::DeepSeek);
}

#[test]
fn route_capability_flags_round_trip_through_provider_config() {
    let caps = CompatCaps::VERBOSITY | CompatCaps::REMOTE_COMPACTION;
    let Ok(encoded) = serde_json::to_value(caps) else {
        panic!("route capabilities must serialize");
    };
    assert_eq!(
        encoded,
        serde_json::json!(["verbosity", "remote_compaction"])
    );

    let Ok(decoded) = serde_json::from_value::<CompatCaps>(encoded) else {
        panic!("route capabilities must deserialize");
    };
    assert!(decoded.contains(CompatCaps::VERBOSITY));
    assert!(decoded.contains(CompatCaps::REMOTE_COMPACTION));
}

#[test]
fn thinking_levels_follow_model_and_route_contracts() {
    use evotengine::ThinkingLevel::*;

    let opus_4_6 = ModelConfig::anthropic("claude-opus-4-6", "Opus 4.6");
    assert_eq!(opus_4_6.supported_thinking_levels(), vec![
        Off, Minimal, Low, Medium, High, Max
    ]);

    let opus_4_8 = ModelConfig::anthropic("claude-opus-4-8", "Opus 4.8");
    assert_eq!(opus_4_8.supported_thinking_levels(), vec![
        Off, Minimal, Low, Medium, High, Xhigh, Max
    ]);

    let gpt_5_5 = ModelConfig::openai("gpt-5.5", "GPT-5.5");
    assert_eq!(gpt_5_5.supported_thinking_levels(), vec![
        Off, Low, Medium, High, Xhigh
    ]);

    let gpt_5_5_pro = ModelConfig::openai("gpt-5.5-pro", "GPT-5.5 Pro");
    assert_eq!(gpt_5_5_pro.supported_thinking_levels(), vec![
        Medium, High, Xhigh
    ]);
    assert_eq!(gpt_5_5_pro.clamp_thinking_level(Low), Medium);
    assert_eq!(gpt_5_5_pro.effective_thinking_level(Off), Medium);

    let xai_route = resolved(
        ApiProtocol::OpenAiCompletions,
        "xai",
        "grok-4.5",
        "https://api.x.ai/v1",
        Some(OpenAiCompat::xai()),
        RouteCapabilities::default(),
        ModelOverrides::default(),
    );
    assert!(xai_route.reasoning());
    assert!(!xai_route.honors_reasoning_effort());
    assert!(xai_route.supported_thinking_levels().is_empty());
}

#[test]
fn api_protocol_display_is_stable() {
    assert_eq!(
        ApiProtocol::AnthropicMessages.to_string(),
        "anthropic_messages"
    );
    assert_eq!(ApiProtocol::OpenAiResponses.to_string(), "openai_responses");
    assert_eq!(
        ApiProtocol::OpenAiCompletions.to_string(),
        "openai_completions"
    );
    assert_eq!(
        ApiProtocol::BedrockConverseStream.to_string(),
        "bedrock_converse_stream"
    );
}
