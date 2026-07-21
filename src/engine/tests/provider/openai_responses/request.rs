use evotengine::provider::openai_responses::request::build_request_body;
use evotengine::provider::ModelConfig;
use evotengine::types::*;

use super::super::fixtures::stream_config::*;

#[test]
fn native_openai_request_uses_responses_schema_with_reasoning_and_tools() {
    let config = StreamConfigBuilder::openai()
        .model("gpt-5.5")
        .model_config(ModelConfig::openai_responses("gpt-5.5", "GPT-5.5"))
        .system_prompt("Be helpful")
        .messages(vec![Message::user("List files")])
        .tools(vec![tool_def("bash", "Run a command")])
        .thinking(ThinkingLevel::Adaptive)
        .build();

    let body = build_request_body(&config);
    assert_eq!(body["model"], "gpt-5.5");
    assert_eq!(body["input"][0]["role"], "developer");
    assert_eq!(body["input"][1]["content"][0]["type"], "input_text");
    assert_eq!(body["tools"][0]["name"], "bash");
    assert!(body["tools"][0].get("function").is_none());
    assert_eq!(body["reasoning"]["effort"], "medium");
    assert_eq!(body["reasoning"]["summary"], "auto");
    assert!(body.get("messages").is_none());
    assert!(body.get("reasoning_effort").is_none());
    assert!(body["max_output_tokens"].is_number());
    assert_eq!(body["store"], false);
}

#[test]
fn first_party_gpt_5_6_responses_off_sends_none_effort() {
    let config = StreamConfigBuilder::openai()
        .model("gpt-5.6-sol")
        .model_config(ModelConfig::openai_responses("gpt-5.6-sol", "GPT-5.6 Sol"))
        .thinking(ThinkingLevel::Off)
        .build();

    let body = build_request_body(&config);
    assert_eq!(body["reasoning"]["effort"], "none");
    assert!(body["reasoning"].get("summary").is_none());
}

#[test]
fn github_copilot_responses_off_omits_reasoning() {
    // No first-party `off -> "none"` mapping is composed for non-openai
    // providers, so Off omits the reasoning field entirely.
    let model = ModelConfig::resolve(
        evotengine::provider::ApiProtocol::OpenAiResponses,
        "github-copilot",
        "gpt-5.6-sol",
        "GPT-5.6 Sol",
        "https://api.githubcopilot.com",
        Some(evotengine::provider::OpenAiCompat::openai()),
    );
    let config = StreamConfigBuilder::openai()
        .model("gpt-5.6-sol")
        .model_config(model)
        .thinking(ThinkingLevel::Off)
        .build();

    let body = build_request_body(&config);
    assert!(body.get("reasoning").is_none());
    assert!(body.get("include").is_none());
}

#[test]
fn responses_off_outside_none_whitelist_omits_reasoning() {
    // A first-party Responses model that is not in the `off -> "none"`
    // whitelist must omit the field rather than send an unsupported "none".
    let config = StreamConfigBuilder::openai()
        .model("gpt-5.7-nova")
        .model_config(ModelConfig::openai_responses(
            "gpt-5.7-nova",
            "GPT-5.7 Nova",
        ))
        .thinking(ThinkingLevel::Off)
        .build();

    let body = build_request_body(&config);
    assert!(body.get("reasoning").is_none());
}
#[test]
fn responses_replays_function_call_and_tool_output() {
    let assistant = Message::Assistant {
        content: vec![
            Content::Thinking {
                thinking: "summary".into(),
                metadata: Some(ThinkingMetadata::OpenAiResponses {
                    item: serde_json::json!({
                        "type": "reasoning",
                        "id": "rs_1",
                        "summary": [{"type": "summary_text", "text": "summary"}],
                        "encrypted_content": "enc",
                    }),
                }),
            },
            Content::ToolCall {
                id: "call_1|fc_1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "pwd"}),
            },
        ],
        stop_reason: StopReason::ToolUse,
        model: "gpt-5.5".into(),
        provider: "openai".into(),
        usage: Usage::default(),
        timestamp: 1,
        error_message: None,
        response_id: None,
    };
    let result = Message::ToolResult {
        tool_call_id: "call_1|fc_1".into(),
        tool_name: "bash".into(),
        content: vec![Content::Text {
            text: "/tmp".into(),
        }],
        is_error: false,
        timestamp: 2,
        retention: Retention::Normal,
    };
    let config = StreamConfigBuilder::openai()
        .messages(vec![assistant, result])
        .build();

    let body = build_request_body(&config);
    assert_eq!(body["input"][0]["type"], "reasoning");
    assert_eq!(body["input"][0]["id"], "rs_1");
    assert_eq!(body["input"][1]["type"], "function_call");
    assert_eq!(body["input"][1]["id"], "fc_1");
    assert_eq!(body["input"][1]["call_id"], "call_1");
    assert_eq!(body["input"][2]["type"], "function_call_output");
    assert_eq!(body["input"][2]["call_id"], "call_1");
    assert_eq!(body["input"][2]["output"], "/tmp");
}
