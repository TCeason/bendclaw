use evotengine::provider::anthropic::request::*;
use evotengine::provider::model::ModelConfig;
use evotengine::provider::traits::*;
use evotengine::types::*;

use super::super::fixtures::stream_config::*;

/// Helper: assistant message with text content.
fn assistant(text: &str) -> Message {
    Message::Assistant {
        content: vec![Content::Text { text: text.into() }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    }
}

/// Helper: multi-turn config used by cache tests.
fn cache_config(cache: CacheConfig) -> StreamConfig {
    StreamConfigBuilder::anthropic()
        .system_prompt("You are helpful.")
        .messages(vec![
            Message::user("Hello"),
            assistant("Hi there!"),
            Message::user("What is 2+2?"),
        ])
        .tools(vec![tool_def("bash", "Run commands")])
        .cache_config(cache)
        .build()
}

// ---------------------------------------------------------------------------
// Thinking
// ---------------------------------------------------------------------------

#[test]
fn test_kimi_coding_request_uses_pi_catalog_limits() {
    let config = StreamConfigBuilder::anthropic()
        .model("kimi-for-coding")
        .model_config(ModelConfig::anthropic("kimi-for-coding", "Kimi For Coding"))
        .no_max_tokens()
        .thinking(ThinkingLevel::Adaptive)
        .build();

    let body = build_request_body(&config, false);
    assert_eq!(body["max_tokens"], 32_768);
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["output_config"]["effort"], "medium");
}

#[test]
fn test_adaptive_thinking_sent_for_anthropic() {
    let config = StreamConfigBuilder::anthropic()
        .thinking(ThinkingLevel::Adaptive)
        .build();

    let body = build_request_body(&config, false);
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["thinking"]["display"], "summarized");
    assert!(body["thinking"].get("budget_tokens").is_none());
    // Adaptive is bounded with medium effort (matches pi's default).
    assert_eq!(body["output_config"]["effort"], "medium");
}

#[test]
fn test_non_off_thinking_sent_as_adaptive_for_anthropic() {
    let config = StreamConfigBuilder::anthropic()
        .thinking(ThinkingLevel::High)
        .build();

    let body = build_request_body(&config, false);
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["thinking"]["display"], "summarized");
    assert!(body["thinking"].get("budget_tokens").is_none());
    assert_eq!(body["output_config"]["effort"], "high");
}

#[test]
fn test_thinking_effort_levels_map_for_anthropic() {
    let cases = [
        (ThinkingLevel::Minimal, "low"),
        (ThinkingLevel::Low, "low"),
        (ThinkingLevel::Medium, "medium"),
        (ThinkingLevel::High, "high"),
        (ThinkingLevel::Adaptive, "medium"),
    ];
    for (level, expected) in cases {
        let config = StreamConfigBuilder::anthropic().thinking(level).build();
        let body = build_request_body(&config, false);
        assert_eq!(
            body["output_config"]["effort"], expected,
            "level {level:?} should map to effort {expected}"
        );
        assert_eq!(body["thinking"]["type"], "adaptive");
    }
}

#[test]
fn test_xhigh_defaults_to_xhigh_effort_without_model_config() {
    // With no model config, the strongest level falls back to "xhigh".
    let config = StreamConfigBuilder::anthropic()
        .thinking(ThinkingLevel::Xhigh)
        .build();
    let body = build_request_body(&config, false);
    assert_eq!(body["output_config"]["effort"], "xhigh");
    assert_eq!(body["thinking"]["type"], "adaptive");
}

#[test]
fn test_xhigh_maps_to_max_on_opus_4_6() {
    // Opus 4.6 only accepts "max" for the strongest effort.
    let config = StreamConfigBuilder::anthropic()
        .model("claude-opus-4-6")
        .model_config(ModelConfig::anthropic("claude-opus-4-6", "Claude Opus 4.6"))
        .thinking(ThinkingLevel::Xhigh)
        .build();
    let body = build_request_body(&config, false);
    assert_eq!(
        body["output_config"]["effort"], "max",
        "Opus 4.6 xhigh should emit max"
    );
}

#[test]
fn test_max_maps_to_max_on_supported_anthropic_model() {
    let config = StreamConfigBuilder::anthropic()
        .model("claude-opus-4-8")
        .model_config(ModelConfig::anthropic("claude-opus-4-8", "Claude Opus 4.8"))
        .thinking(ThinkingLevel::Max)
        .build();
    let body = build_request_body(&config, false);
    assert_eq!(body["output_config"]["effort"], "max");
}

#[test]
fn test_xhigh_maps_to_xhigh_on_opus_4_8() {
    // Opus 4.7+/4.8 use "xhigh" (max is invalid there).
    let config = StreamConfigBuilder::anthropic()
        .model("claude-opus-4-8")
        .model_config(ModelConfig::anthropic("claude-opus-4-8", "Claude Opus 4.8"))
        .thinking(ThinkingLevel::Xhigh)
        .build();
    let body = build_request_body(&config, false);
    assert_eq!(
        body["output_config"]["effort"], "xhigh",
        "Opus 4.8 xhigh should emit xhigh"
    );
}

#[test]
fn test_xhigh_maps_to_max_on_date_suffixed_opus_4_6() {
    // Real ids carry a date suffix; version parsing must still resolve 4.6.
    let id = "claude-opus-4-6-20251101";
    let config = StreamConfigBuilder::anthropic()
        .model(id)
        .model_config(ModelConfig::anthropic(id, "Claude Opus 4.6"))
        .thinking(ThinkingLevel::Xhigh)
        .build();
    let body = build_request_body(&config, false);
    assert_eq!(
        body["output_config"]["effort"], "max",
        "date-suffixed Opus 4.6 xhigh should still emit max"
    );
}

#[test]
fn test_xhigh_defaults_on_future_opus_via_version_gate() {
    // A hypothetical newer Opus needs no table entry: it falls through to xhigh.
    let id = "claude-opus-5-0";
    let config = StreamConfigBuilder::anthropic()
        .model(id)
        .model_config(ModelConfig::anthropic(id, "Claude Opus 5"))
        .thinking(ThinkingLevel::Xhigh)
        .build();
    let body = build_request_body(&config, false);
    assert_eq!(
        body["output_config"]["effort"], "xhigh",
        "future Opus xhigh should default to xhigh"
    );
}

#[test]
fn test_off_thinking_disables_anthropic_thinking() {
    let config = StreamConfigBuilder::anthropic()
        .thinking(ThinkingLevel::Off)
        .build();

    let body = build_request_body(&config, false);
    // Off explicitly disables thinking (mirrors pi's `{ type: "disabled" }`)
    // rather than omitting the field and falling back to the model default.
    assert_eq!(body["thinking"]["type"], "disabled");
    // Disabled thinking carries no effort bound.
    assert!(body.get("output_config").is_none());
}

#[test]
fn test_off_thinking_omitted_when_model_cannot_disable() {
    // A model that maps `off` to None cannot have reasoning turned off, so the
    // thinking field is omitted entirely instead of sending `disabled`.
    let mut model_config = ModelConfig::anthropic("claude-fable-5", "Fable 5");
    model_config.thinking_level_map.insert("off".into(), None);
    let config = StreamConfigBuilder::anthropic()
        .model_config(model_config)
        .thinking(ThinkingLevel::Off)
        .build();

    let body = build_request_body(&config, false);
    assert!(body.get("thinking").is_none());
    assert!(body.get("output_config").is_none());
}

#[test]
fn test_anthropic_max_tokens_falls_back_to_model_config() {
    // No explicit max_tokens: use the model's configured budget. Modern Claude
    // 4.x supports a 64k output budget, so a small default would truncate long
    // responses. The request builder still clamps this to the window per call.
    let config = StreamConfigBuilder::anthropic()
        .no_max_tokens()
        .model_config(ModelConfig::anthropic(
            "claude-sonnet-4-20250514",
            "Claude Sonnet 4",
        ))
        .build();

    let body = build_request_body(&config, false);
    assert_eq!(body["max_tokens"], 64000);
}

#[test]
fn test_anthropic_max_tokens_clamped_to_remaining_context() {
    // A near-full context window must shrink the output budget so the request
    // never overflows the window (which providers reject). Mirrors pi's
    // clampMaxTokensToContext.
    let mut model_config = ModelConfig::anthropic("claude-sonnet-4-20250514", "Claude Sonnet 4");
    model_config.context_window = 10_000;
    // ~8000 tokens of input (byte-length / 4) leaves < 64k of headroom.
    let big_message = "x".repeat(32_000);
    let config = StreamConfigBuilder::anthropic()
        .no_max_tokens()
        .messages(vec![Message::user(big_message)])
        .model_config(model_config)
        .build();

    let body = build_request_body(&config, false);
    let max_tokens = body["max_tokens"].as_u64().unwrap();
    // context_window(10_000) - input(~8000) - safety(4096) is negative, so the
    // clamp floors at 1 rather than sending a budget larger than the window.
    assert!(max_tokens >= 1, "got {max_tokens}");
    assert!(
        max_tokens < 64_000,
        "expected clamp below model cap, got {max_tokens}"
    );
}

#[test]
fn test_anthropic_max_tokens_uses_large_budget_for_supported_model() {
    // Opus 4.6+ genuinely supports a 128000 output budget, so the fallback
    // should honor it when the model config advertises it.
    let config = StreamConfigBuilder::anthropic()
        .no_max_tokens()
        .model_config(ModelConfig::anthropic("claude-opus-4-6", "Claude Opus 4.6"))
        .build();

    let body = build_request_body(&config, false);
    assert_eq!(body["max_tokens"], 128000);
}

#[test]
fn test_anthropic_explicit_max_tokens_wins() {
    let config = StreamConfigBuilder::anthropic()
        .max_tokens(4096)
        .model_config(ModelConfig::anthropic("claude-opus-4-6", "Claude Opus 4.6"))
        .build();

    let body = build_request_body(&config, false);
    assert_eq!(body["max_tokens"], 4096);
}

#[test]
fn test_anthropic_max_tokens_default_without_model_config() {
    // No max_tokens and no model config: fall back to a conservative default
    // (8192) instead of the previous oversized 128000 constant.
    let config = StreamConfigBuilder::anthropic().no_max_tokens().build();

    let body = build_request_body(&config, false);
    assert_eq!(body["max_tokens"], 8192);
}

// ---------------------------------------------------------------------------
// Cache breakpoint tests
// ---------------------------------------------------------------------------

#[test]
fn test_cache_auto_places_all_breakpoints() {
    let body = build_request_body(&cache_config(CacheConfig::default()), false);

    let system = &body["system"][0];
    assert_eq!(system["cache_control"]["type"], "ephemeral");

    let tools = body["tools"].as_array().unwrap();
    let last_tool = tools.last().unwrap();
    assert_eq!(last_tool["cache_control"]["type"], "ephemeral");

    let msgs = body["messages"].as_array().unwrap();
    let second_to_last = &msgs[msgs.len() - 2];
    let content = second_to_last["content"].as_array().unwrap();
    let last_block = content.last().unwrap();
    assert_eq!(last_block["cache_control"]["type"], "ephemeral");
}

#[test]
fn test_system_prompt_boundary_splits_dynamic_part_without_cache_control() {
    let config = StreamConfigBuilder::anthropic()
        .system_prompt("stable\n__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__\ndynamic")
        .build();

    let body = build_request_body(&config, false);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 2);
    assert_eq!(system[0]["text"], "stable");
    assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    assert_eq!(system[1]["text"], "dynamic");
    assert!(system[1].get("cache_control").is_none());
}

#[test]
fn test_system_prompt_boundary_uses_last_marker() {
    let config = StreamConfigBuilder::anthropic()
        .system_prompt(
            "project mentions __SYSTEM_PROMPT_DYNAMIC_BOUNDARY__ literally\n__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__\ndynamic",
        )
        .build();

    let body = build_request_body(&config, false);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 2);
    assert_eq!(
        system[0]["text"],
        "project mentions __SYSTEM_PROMPT_DYNAMIC_BOUNDARY__ literally"
    );
    assert_eq!(system[1]["text"], "dynamic");
}

#[test]
fn test_oauth_system_prompt_boundary_preserves_prelude() {
    let config = StreamConfigBuilder::anthropic()
        .system_prompt("stable\n__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__\ndynamic")
        .build();

    let body = build_request_body(&config, true);
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 3);
    assert_eq!(
        system[0]["text"],
        "You are Claude Code, Anthropic's official CLI for Claude."
    );
    assert!(system[0].get("cache_control").is_none());
    assert_eq!(system[1]["text"], "stable");
    assert_eq!(system[1]["cache_control"]["type"], "ephemeral");
    assert_eq!(system[2]["text"], "dynamic");
    assert!(system[2].get("cache_control").is_none());
}

#[test]
fn test_cache_disabled_no_breakpoints() {
    let config = CacheConfig {
        enabled: false,
        strategy: CacheStrategy::Auto,
    };
    let body = build_request_body(&cache_config(config), false);

    let system = &body["system"][0];
    assert!(system.get("cache_control").is_none());

    let tools = body["tools"].as_array().unwrap();
    assert!(tools.last().unwrap().get("cache_control").is_none());

    let msgs = body["messages"].as_array().unwrap();
    for msg in msgs {
        if let Some(content) = msg["content"].as_array() {
            for block in content {
                assert!(block.get("cache_control").is_none());
            }
        }
    }
}

#[test]
fn test_cache_manual_system_only() {
    let config = CacheConfig {
        enabled: true,
        strategy: CacheStrategy::Manual {
            cache_system: true,
            cache_tools: false,
            cache_messages: false,
        },
    };
    let body = build_request_body(&cache_config(config), false);

    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert!(body["tools"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .get("cache_control")
        .is_none());
    let msgs = body["messages"].as_array().unwrap();
    let second = &msgs[msgs.len() - 2];
    let content = second["content"].as_array().unwrap();
    assert!(content.last().unwrap().get("cache_control").is_none());
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

#[test]
fn test_usage_cache_hit_rate() {
    let usage = Usage {
        input: 100,
        output: 50,
        cache_read: 900,
        cache_write: 0,
        total_tokens: 1050,
        reasoning_output: 0,
    };
    let rate = usage.cache_hit_rate();
    assert!((rate - 0.9).abs() < 0.001);

    let empty = Usage::default();
    assert_eq!(empty.cache_hit_rate(), 0.0);
}

// ---------------------------------------------------------------------------
// Tool result serialization
// ---------------------------------------------------------------------------

#[test]
fn test_tool_result_with_image() {
    let config = StreamConfigBuilder::anthropic()
        .cache_disabled()
        .messages(vec![
            Message::Assistant {
                content: vec![Content::ToolCall {
                    id: "tc-1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path": "test.png"}),
                }],
                stop_reason: StopReason::ToolUse,
                model: "test".into(),
                provider: "test".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
                response_id: None,
            },
            Message::ToolResult {
                tool_call_id: "tc-1".into(),
                tool_name: "read".into(),
                content: vec![
                    Content::Text {
                        text: "screenshot".into(),
                    },
                    Content::Image {
                        mime_type: "image/png".into(),
                        source: ImageSource::Base64 {
                            data: "aW1hZ2VkYXRh".into(),
                            path: None,
                        },
                    },
                ],
                is_error: false,
                timestamp: 0,
                retention: Retention::Normal,
            },
        ])
        .build();

    let body = build_request_body(&config, false);
    let msgs = body["messages"].as_array().unwrap();
    let tool_msg = &msgs[1];
    let tool_result = &tool_msg["content"][0];
    assert_eq!(tool_result["type"], "tool_result");
    let content = tool_result["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
}

#[test]
fn test_tool_result_text_only_uses_string() {
    let config = StreamConfigBuilder::anthropic()
        .cache_disabled()
        .messages(vec![
            Message::Assistant {
                content: vec![Content::ToolCall {
                    id: "tc-1".into(),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "echo hi"}),
                }],
                stop_reason: StopReason::ToolUse,
                model: "test".into(),
                provider: "test".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
                response_id: None,
            },
            Message::ToolResult {
                tool_call_id: "tc-1".into(),
                tool_name: "bash".into(),
                content: vec![Content::Text {
                    text: "hello".into(),
                }],
                is_error: false,
                timestamp: 0,
                retention: Retention::Normal,
            },
        ])
        .build();

    let body = build_request_body(&config, false);
    let msgs = body["messages"].as_array().unwrap();
    let tool_result = &msgs[1]["content"][0];
    assert_eq!(tool_result["content"], "hello");
}

// ---------------------------------------------------------------------------
// Content filtering
// ---------------------------------------------------------------------------

#[test]
fn test_content_to_anthropic_keeps_native_signature_but_downgrades_openai_field_marker() {
    let content = vec![
        Content::Thinking {
            thinking: "native".into(),
            metadata: Some(ThinkingMetadata::Anthropic {
                signature: "cryptographic-signature".into(),
            }),
        },
        Content::Thinking {
            thinking: "cross-provider".into(),
            metadata: Some(ThinkingMetadata::OpenAiCompletions {
                field: ReasoningField::ReasoningContent,
            }),
        },
        Content::Thinking {
            thinking: "unsigned".into(),
            metadata: None,
        },
    ];

    let result = content_to_anthropic(&content);

    assert_eq!(result[0]["type"], "thinking");
    assert_eq!(result[0]["signature"], "cryptographic-signature");
    assert_eq!(result[1]["type"], "text");
    assert_eq!(result[1]["text"], "cross-provider");
    assert_eq!(result[2]["type"], "text");
    assert_eq!(result[2]["text"], "unsigned");
}

#[test]
fn test_content_to_anthropic_filters_empty_text() {
    let content = vec![
        Content::Text { text: "".into() },
        Content::Text {
            text: "hello".into(),
        },
        Content::Text { text: "".into() },
    ];
    let result = content_to_anthropic(&content);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["text"], "hello");
}

// ---------------------------------------------------------------------------
// Cache breakpoint edge cases (empty content blocks)
// ---------------------------------------------------------------------------

/// Messages with alternating roles including an empty-text user message.
fn empty_text_messages() -> Vec<Message> {
    vec![
        Message::user("first message"),
        assistant("ok"),
        Message::User {
            content: vec![Content::Text { text: "".into() }],
            timestamp: 0,
        },
        assistant("sure"),
        Message::user("last"),
    ]
}

#[test]
fn test_cache_control_not_set_on_empty_text_block() {
    let config = StreamConfigBuilder::anthropic()
        .system_prompt("You are helpful.")
        .messages(empty_text_messages())
        .build();

    let body = build_request_body(&config, false);
    let msgs = body["messages"].as_array().unwrap();

    let empty_msg = &msgs[2];
    let content = empty_msg["content"].as_array().unwrap();
    assert!(
        content.is_empty(),
        "empty text blocks should be filtered out"
    );

    let cached_msg = &msgs[3];
    let cached_content = cached_msg["content"].as_array().unwrap();
    let last_block = cached_content.last().unwrap();
    assert_eq!(
        last_block["cache_control"]["type"], "ephemeral",
        "cache_control should land on the second-to-last message with content"
    );
}

#[test]
fn test_cache_breakpoint_falls_back_when_second_to_last_is_empty() {
    let config = StreamConfigBuilder::anthropic()
        .system_prompt("You are helpful.")
        .messages(vec![
            Message::user("first message"),
            assistant("ok"),
            Message::User {
                content: vec![Content::Text { text: "".into() }],
                timestamp: 0,
            },
            assistant("sure"),
            Message::user("last message"),
        ])
        .build();

    let body = build_request_body(&config, false);
    let msgs = body["messages"].as_array().unwrap();

    let cached_content = msgs[3]["content"].as_array().unwrap();
    assert_eq!(
        cached_content.last().unwrap()["cache_control"]["type"],
        "ephemeral"
    );
}

#[test]
fn test_empty_assistant_preserved_as_placeholder() {
    let config = StreamConfigBuilder::anthropic()
        .cache_disabled()
        .messages(vec![
            Message::user("first"),
            Message::Assistant {
                content: vec![Content::Text { text: "".into() }],
                stop_reason: StopReason::Error,
                model: "test".into(),
                provider: "test".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: Some("Empty response".into()),
                response_id: None,
            },
            Message::user("second"),
        ])
        .build();

    let body = build_request_body(&config, false);
    let msgs = body["messages"].as_array().unwrap();

    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[2]["role"], "user");

    let assistant_content = msgs[1]["content"].as_array().unwrap();
    assert_eq!(assistant_content[0]["text"], "[empty response]");
}

// ---------------------------------------------------------------------------
// build_messages_url
// ---------------------------------------------------------------------------

#[test]
fn messages_url_appends_v1_for_bare_base() {
    assert_eq!(
        build_messages_url("https://api.anthropic.com"),
        "https://api.anthropic.com/v1/messages"
    );
}

#[test]
fn messages_url_trims_trailing_slash() {
    assert_eq!(
        build_messages_url("https://api.anthropic.com/"),
        "https://api.anthropic.com/v1/messages"
    );
}

#[test]
fn messages_url_does_not_double_v1_suffix() {
    assert_eq!(
        build_messages_url("https://gateway.example.com/anthropic/v1"),
        "https://gateway.example.com/anthropic/v1/messages"
    );
    assert_eq!(
        build_messages_url("https://gateway.example.com/anthropic/v1/"),
        "https://gateway.example.com/anthropic/v1/messages"
    );
}

#[test]
fn messages_url_keeps_v1_only_at_path_end() {
    assert_eq!(
        build_messages_url("https://example.com/v1/proxy"),
        "https://example.com/v1/proxy/v1/messages"
    );
}
