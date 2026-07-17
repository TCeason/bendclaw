use evotengine::context::transform_messages_for_model;
use evotengine::provider::ApiProtocol;
use evotengine::Content;
use evotengine::Message;
use evotengine::ReasoningField;
use evotengine::StopReason;
use evotengine::ThinkingMetadata;
use evotengine::Usage;

fn assistant(provider: &str, model: &str, content: Vec<Content>) -> Message {
    Message::Assistant {
        content,
        stop_reason: StopReason::Stop,
        model: model.into(),
        provider: provider.into(),
        usage: Usage::default(),
        timestamp: 1,
        error_message: None,
        response_id: None,
    }
}

#[test]
fn same_model_and_api_preserve_replayable_thinking_metadata() {
    let message = assistant("anthropic", "claude", vec![Content::Thinking {
        thinking: "plan".into(),
        metadata: Some(ThinkingMetadata::Anthropic {
            signature: "sig".into(),
        }),
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "anthropic",
        "claude",
        ApiProtocol::AnthropicMessages,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::Thinking {
                metadata: Some(ThinkingMetadata::Anthropic { signature }), ..
            }] if signature == "sig")
    ));
}

#[test]
fn same_openai_responses_model_preserves_reasoning_item() {
    let message = assistant("openai", "gpt-5.5", vec![Content::Thinking {
        thinking: "plan".into(),
        metadata: Some(ThinkingMetadata::OpenAiResponses {
            item: serde_json::json!({
                "type": "reasoning",
                "id": "rs_1",
                "encrypted_content": "enc",
                "summary": [],
            }),
        }),
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "openai",
        "gpt-5.5",
        ApiProtocol::OpenAiResponses,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::Thinking {
                metadata: Some(ThinkingMetadata::OpenAiResponses { item }), ..
            }] if item["id"] == "rs_1")
    ));
}

#[test]
fn cross_openai_responses_model_drops_function_item_id() {
    let message = assistant("openai", "old-model", vec![Content::ToolCall {
        id: "call_1|fc_1".into(),
        name: "bash".into(),
        arguments: serde_json::json!({"command": "pwd"}),
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "openai",
        "new-model",
        ApiProtocol::OpenAiResponses,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::ToolCall { id, .. }] if id == "call_1")
    ));
}

#[test]
fn same_openai_responses_model_preserves_function_item_id() {
    let message = assistant("openai", "gpt-5.5", vec![Content::ToolCall {
        id: "call_1|fc_1".into(),
        name: "bash".into(),
        arguments: serde_json::json!({"command": "pwd"}),
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "openai",
        "gpt-5.5",
        ApiProtocol::OpenAiResponses,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::ToolCall { id, .. }] if id == "call_1|fc_1")
    ));
}

#[test]
fn cross_model_thinking_is_downgraded_to_text() {
    let message = assistant("openai", "old-model", vec![Content::Thinking {
        thinking: "useful plan".into(),
        metadata: Some(ThinkingMetadata::OpenAiCompletions {
            field: ReasoningField::ReasoningContent,
        }),
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "openai",
        "new-model",
        ApiProtocol::OpenAiCompletions,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::Text { text }] if text == "useful plan")
    ));
}

#[test]
fn foreign_protocol_metadata_is_downgraded_even_when_names_match() {
    let message = assistant("proxy", "model", vec![Content::Thinking {
        thinking: "plan".into(),
        metadata: Some(ThinkingMetadata::Anthropic {
            signature: "sig".into(),
        }),
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "proxy",
        "model",
        ApiProtocol::OpenAiCompletions,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::Text { text }] if text == "plan")
    ));
}

#[test]
fn unsigned_same_model_thinking_is_downgraded_to_text() {
    let message = assistant("anthropic", "claude", vec![Content::Thinking {
        thinking: "plan".into(),
        metadata: None,
    }]);

    let transformed = transform_messages_for_model(
        vec![message],
        "anthropic",
        "claude",
        ApiProtocol::AnthropicMessages,
    );

    assert!(matches!(
        &transformed[0],
        Message::Assistant { content, .. }
            if matches!(&content[..], [Content::Text { text }] if text == "plan")
    ));
}
