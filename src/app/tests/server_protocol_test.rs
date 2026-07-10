use evot::agent::*;

#[test]
fn transcript_round_trip_preserves_ordered_assistant_blocks_and_signature() {
    let message = evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant {
        content: vec![
            evot_engine::Content::Thinking {
                thinking: "plan".into(),
                metadata: Some(evot_engine::ThinkingMetadata::OpenAiCompletions {
                    field: evot_engine::ReasoningField::Reasoning,
                }),
            },
            evot_engine::Content::ToolCall {
                id: "call-1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": "a"}),
            },
            evot_engine::Content::Text {
                text: "done".into(),
            },
        ],
        stop_reason: evot_engine::StopReason::ToolUse,
        model: "model".into(),
        provider: "provider".into(),
        usage: evot_engine::Usage::default(),
        timestamp: 1,
        error_message: None,
        response_id: None,
    });

    let transcript = evot::agent::run::convert::transcript_from_agent_message(&message);
    let restored = evot::agent::run::convert::agent_message_from_transcript(&transcript);

    assert!(matches!(
        restored,
        evot_engine::AgentMessage::Llm(evot_engine::Message::Assistant { content, .. })
            if matches!(&content[..], [
                evot_engine::Content::Thinking { thinking, metadata },
                evot_engine::Content::ToolCall { id, .. },
                evot_engine::Content::Text { text },
            ] if thinking == "plan"
                && matches!(metadata, Some(evot_engine::ThinkingMetadata::OpenAiCompletions {
                    field: evot_engine::ReasoningField::Reasoning,
                }))
                && id == "call-1"
                && text == "done")
    ));
}

#[test]
fn run_event_round_trip_run_started() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        0,
        RunEventPayload::RunStarted {},
    );
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify top-level shape: { event_id, run_id, session_id, turn, kind, payload, created_at }
    assert_eq!(parsed["kind"], "run_started");
    assert_eq!(parsed["run_id"], "run-1");
    assert_eq!(parsed["session_id"], "sess-1");
    assert_eq!(parsed["turn"], 0);
    assert!(parsed["event_id"].is_string());
    assert!(parsed["created_at"].is_string());
    assert!(parsed["payload"].is_object());

    // Round-trip
    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.run_id, "run-1");
    assert!(matches!(
        deserialized.payload,
        RunEventPayload::RunStarted {}
    ));
}

#[test]
fn run_event_round_trip_assistant_delta_text_only() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantDelta {
            content_index: 2,
            content_type: AssistantContentType::Text,
            delta: "hello".into(),
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["kind"], "assistant_delta");
    assert_eq!(parsed["payload"]["content_index"], 2);
    assert_eq!(parsed["payload"]["content_type"], "text");
    assert_eq!(parsed["payload"]["delta"], "hello");

    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();
    if let RunEventPayload::AssistantDelta {
        content_index,
        content_type,
        delta,
    } = &deserialized.payload
    {
        assert_eq!(*content_index, 2);
        assert!(matches!(content_type, AssistantContentType::Text));
        assert_eq!(delta, "hello");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_event_round_trip_assistant_delta_thinking_only() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantDelta {
            content_index: 0,
            content_type: AssistantContentType::Thinking,
            delta: "hmm".into(),
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["payload"]["content_index"], 0);
    assert_eq!(parsed["payload"]["content_type"], "thinking");
    assert_eq!(parsed["payload"]["delta"], "hmm");

    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();
    if let RunEventPayload::AssistantDelta {
        content_index,
        content_type,
        delta,
    } = &deserialized.payload
    {
        assert_eq!(*content_index, 0);
        assert!(matches!(content_type, AssistantContentType::Thinking));
        assert_eq!(delta, "hmm");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_event_round_trip_assistant_tool_call() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantToolCall {
            content_index: 2,
            tool_call_id: "call-2".into(),
            tool_name: "edit".into(),
            phase: ToolCallStreamPhase::End,
            delta: None,
            args: Some(serde_json::json!({"path": "src/lib.rs"})),
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["kind"], "assistant_tool_call");
    assert_eq!(parsed["payload"]["content_index"], 2);
    assert_eq!(parsed["payload"]["tool_call_id"], "call-2");
    assert_eq!(parsed["payload"]["tool_name"], "edit");
    assert_eq!(parsed["payload"]["phase"], "end");
    assert_eq!(parsed["payload"]["args"]["path"], "src/lib.rs");
}

#[test]
fn run_event_round_trip_assistant_completed() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantCompleted {
            content: vec![
                AssistantBlock::Text { text: "hi".into() },
                AssistantBlock::ToolCall {
                    id: "tc-1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "/tmp"}),
                },
            ],
            usage: Some(UsageSummary {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_write: 0,
            }),
            stop_reason: "toolUse".into(),
            error_message: None,
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();

    if let RunEventPayload::AssistantCompleted {
        content,
        usage,
        stop_reason,
        ..
    } = &deserialized.payload
    {
        assert_eq!(content.len(), 2);
        assert!(usage.is_some());
        assert_eq!(usage.as_ref().unwrap().input, 100);
        assert_eq!(stop_reason, "toolUse");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_event_round_trip_tool_finished() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::ToolFinished {
            tool_call_id: "tc-1".into(),
            tool_name: "read".into(),
            content: "file contents".into(),
            is_error: false,
            details: serde_json::Value::Null,
            result_tokens: 3,
            duration_ms: 100,
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["kind"], "tool_finished");
    assert_eq!(parsed["payload"]["tool_name"], "read");
    assert_eq!(parsed["payload"]["is_error"], false);

    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(
        deserialized.payload,
        RunEventPayload::ToolFinished { .. }
    ));
}

#[test]
fn run_event_round_trip_llm_call_retry() -> Result<(), Box<dyn std::error::Error>> {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::LlmCallRetry {
            turn: 1,
            attempt: 2,
            max_retries: 3,
            delay_ms: 2100,
            error: "tls handshake eof".into(),
        },
    );
    let json = serde_json::to_string(&event)?;
    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(parsed["kind"], "llm_call_retry");
    assert_eq!(parsed["payload"]["attempt"], 2);
    assert_eq!(parsed["payload"]["delay_ms"], 2100);

    let deserialized: RunEvent = serde_json::from_str(&json)?;
    if let RunEventPayload::LlmCallRetry {
        attempt,
        max_retries,
        delay_ms,
        error,
        ..
    } = &deserialized.payload
    {
        assert_eq!(*attempt, 2);
        assert_eq!(*max_retries, 3);
        assert_eq!(*delay_ms, 2100);
        assert_eq!(error, "tls handshake eof");
    } else {
        panic!("wrong variant");
    }
    Ok(())
}

#[test]
fn run_event_round_trip_run_finished() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        2,
        RunEventPayload::RunFinished {
            text: "done".into(),
            usage: UsageSummary {
                input: 200,
                output: 100,
                cache_read: 0,
                cache_write: 0,
            },
            turn_count: 2,
            duration_ms: 1500,
            transcript_count: 4,
            compact_history: vec![],
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();

    if let RunEventPayload::RunFinished {
        turn_count,
        duration_ms,
        usage,
        ..
    } = &deserialized.payload
    {
        assert_eq!(*turn_count, 2);
        assert_eq!(*duration_ms, 1500);
        assert_eq!(usage.input, 200);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_event_round_trip_error() {
    let event = RunEvent::new("run-1".into(), "sess-1".into(), 0, RunEventPayload::Error {
        message: "bad request".into(),
    });
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: RunEvent = serde_json::from_str(&json).unwrap();
    if let RunEventPayload::Error { message } = &deserialized.payload {
        assert_eq!(message, "bad request");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn run_event_deserialize_rejects_missing_fields() {
    // Missing event_id
    let bad_json = r#"{"run_id":"r","session_id":"s","turn":0,"kind":"run_started","payload":{},"created_at":"t"}"#;
    let result = serde_json::from_str::<RunEvent>(bad_json);
    assert!(result.is_err());

    // Missing kind
    let bad_json2 =
        r#"{"event_id":"e","run_id":"r","session_id":"s","turn":0,"payload":{},"created_at":"t"}"#;
    let result2 = serde_json::from_str::<RunEvent>(bad_json2);
    assert!(result2.is_err());

    // Missing run_id
    let bad_json3 = r#"{"event_id":"e","session_id":"s","turn":0,"kind":"run_started","payload":{},"created_at":"t"}"#;
    let result3 = serde_json::from_str::<RunEvent>(bad_json3);
    assert!(result3.is_err());

    // Missing payload
    let bad_json4 = r#"{"event_id":"e","run_id":"r","session_id":"s","turn":0,"kind":"run_started","created_at":"t"}"#;
    let result4 = serde_json::from_str::<RunEvent>(bad_json4);
    assert!(result4.is_err());
}

// ---------------------------------------------------------------------------
// SSE mapping tests (server/stream.rs::map_run_event_json)
// ---------------------------------------------------------------------------

use evot::gateway::channels::http::stream::map_run_event_json;

#[test]
fn sse_map_assistant_delta() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantDelta {
            content_index: 0,
            content_type: AssistantContentType::Text,
            delta: "hi".into(),
        },
    );
    let payloads = map_run_event_json(&event);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["type"], "text");
    assert_eq!(payloads[0]["data"]["text"], "hi");
}

#[test]
fn sse_map_assistant_tool_call() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantToolCall {
            content_index: 1,
            tool_call_id: "tc-1".into(),
            tool_name: "edit".into(),
            phase: ToolCallStreamPhase::Delta,
            delta: Some("{\"path\":\"/tmp/a\"}".into()),
            args: None,
        },
    );
    let payloads = map_run_event_json(&event);

    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["type"], "tool_call_delta");
    assert_eq!(payloads[0]["data"]["id"], "tc-1");
    assert_eq!(payloads[0]["data"]["name"], "edit");
    assert_eq!(payloads[0]["data"]["phase"], "delta");
    assert_eq!(payloads[0]["data"]["delta"], "{\"path\":\"/tmp/a\"}");
    assert!(payloads[0]["data"]["input"].is_null());
}

#[test]
fn sse_map_tool_call_from_assistant_completed() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantCompleted {
            content: vec![
                AssistantBlock::Text {
                    text: "thinking".into(),
                },
                AssistantBlock::ToolCall {
                    id: "tc-1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "/tmp"}),
                },
            ],
            usage: None,
            stop_reason: "toolUse".into(),
            error_message: None,
        },
    );
    let payloads = map_run_event_json(&event);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["type"], "tool_call");
    assert_eq!(payloads[0]["data"]["name"], "read");
}

#[test]
fn sse_map_tool_result() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::ToolFinished {
            tool_call_id: "tc-1".into(),
            tool_name: "read".into(),
            content: "file data".into(),
            is_error: false,
            details: serde_json::Value::Null,
            result_tokens: 2,
            duration_ms: 80,
        },
    );
    let payloads = map_run_event_json(&event);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["type"], "tool_result");
    assert_eq!(payloads[0]["data"]["content"], "file data");
    assert_eq!(payloads[0]["data"]["is_error"], false);
}

#[test]
fn sse_map_run_finished() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        2,
        RunEventPayload::RunFinished {
            text: "done".into(),
            usage: UsageSummary {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_write: 0,
            },
            turn_count: 2,
            duration_ms: 1500,
            transcript_count: 4,
            compact_history: vec![],
        },
    );
    let payloads = map_run_event_json(&event);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["type"], "result");
    assert_eq!(payloads[0]["data"]["input_tokens"], 100);
    assert_eq!(payloads[0]["data"]["output_tokens"], 50);
    assert_eq!(payloads[0]["data"]["turn_count"], 2);
}

#[test]
fn sse_map_error() {
    let event = RunEvent::new("run-1".into(), "sess-1".into(), 0, RunEventPayload::Error {
        message: "boom".into(),
    });
    let payloads = map_run_event_json(&event);
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["type"], "error");
    assert_eq!(payloads[0]["data"]["message"], "boom");
}

#[test]
fn sse_map_run_started_produces_nothing() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        0,
        RunEventPayload::RunStarted {},
    );
    let payloads = map_run_event_json(&event);
    assert!(payloads.is_empty());
}

// ---------------------------------------------------------------------------
// StreamJsonSink output shape test
// ---------------------------------------------------------------------------

#[test]
fn stream_json_output_preserves_shape() {
    let event = RunEvent::new(
        "run-1".into(),
        "sess-1".into(),
        1,
        RunEventPayload::AssistantDelta {
            content_index: 0,
            content_type: AssistantContentType::Text,
            delta: "hello".into(),
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // StreamJsonSink does serde_json::to_string(event) — verify the shape
    assert!(parsed.get("event_id").is_some());
    assert!(parsed.get("run_id").is_some());
    assert!(parsed.get("session_id").is_some());
    assert!(parsed.get("turn").is_some());
    assert!(parsed.get("kind").is_some());
    assert!(parsed.get("payload").is_some());
    assert!(parsed.get("created_at").is_some());
    // kind is top-level string, not nested
    assert!(parsed["kind"].is_string());
    // payload is object without kind inside
    assert!(parsed["payload"].get("kind").is_none());
}
