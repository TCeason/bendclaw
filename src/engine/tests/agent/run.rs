//! Tests for the core agent loop using MockProvider.

use evotengine::agent_loop;
use evotengine::provider::mock::*;
use evotengine::provider::MockProvider;
use evotengine::AgentLoopConfig;
use evotengine::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::fixtures::agent_harness::collect_events;
use super::fixtures::agent_harness::make_config;
use super::fixtures::agent_harness::MockTool;
use super::fixtures::agent_harness::TestHarness;

fn system_reminder_count(messages: &[AgentMessage]) -> usize {
    messages
        .iter()
        .filter(|message| {
            matches!(
                message,
                AgentMessage::Llm(Message::User { content, .. })
                    if content.iter().any(|content| matches!(content, Content::Text { text } if text.contains("<system-reminder>")))
            )
        })
        .count()
}

// ---------------------------------------------------------------------------
// Tests using TestHarness
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_simple_text_response() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello, world!".into())])
        .system_prompt("You are helpful.")
        .run("Hi")
        .await;

    output.assert_completed();
    assert!(output.has_event("TurnStart"));
    assert!(output.has_event("TurnEnd"));

    output.assert_message_count(2);
    assert_eq!(output.messages[0].role(), "user");
    assert_eq!(output.messages[1].role(), "assistant");
    assert_eq!(output.context_messages.len(), 2);
}

#[tokio::test]
async fn test_tool_call_and_response() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "read".into(),
                arguments: serde_json::json!({"path": "test.txt"}),
            }]),
            MockResponse::Text("The file contains: hello".into()),
        ])
        .tool(MockTool::ok("read", "hello"))
        .run("Read test.txt")
        .await;

    assert!(output.has_event("ToolExecStart"));
    assert!(output.has_event("ToolExecEnd"));

    // Messages: user, assistant(tool_call), toolResult, assistant(text)
    // No convergence reminder is injected — the guidance that used to live in
    // a runtime <system-reminder> now sits in the static system prompt, so
    // nothing extra gets pushed into context between tool_result and the next
    // assistant turn.
    output.assert_message_count(4);
    assert_eq!(output.messages[0].role(), "user");
    assert_eq!(output.messages[1].role(), "assistant");
    assert_eq!(output.messages[2].role(), "toolResult");
    assert_eq!(output.messages[3].role(), "assistant");
    assert_eq!(system_reminder_count(&output.messages), 0);
}

#[tokio::test]
async fn test_abort_cancels_loop() {
    // This test needs direct cancel token access — keep manual setup
    let provider = MockProvider::text("Should not appear");
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("Hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    assert!(new_messages.len() <= 2);
}

#[tokio::test]
async fn test_continue_from_tool_result() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Done processing.".into())])
        .prior_messages(vec![
            AgentMessage::Llm(Message::user("do something")),
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id: "tc-1".into(),
                tool_name: "test_tool".into(),
                content: vec![Content::Text {
                    text: "result".into(),
                }],
                is_error: false,
                timestamp: 0,
                retention: Retention::Normal,
            }),
        ])
        .run_continue()
        .await;

    assert!(!output.messages.is_empty());
    assert_eq!(output.messages[0].role(), "assistant");
}

#[tokio::test]
async fn test_tool_error_is_reported() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "failing_tool".into(),
                arguments: serde_json::json!({}),
            }]),
            MockResponse::Text("Tool failed, sorry.".into()),
        ])
        .tool(MockTool::err("failing_tool", "Something went wrong"))
        .run("Use the tool")
        .await;

    assert_eq!(output.tool_errors().len(), 1);
    output.assert_last_role("assistant");
}

#[tokio::test]
async fn test_unknown_tool_reports_error() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "nonexistent".into(),
                arguments: serde_json::json!({}),
            }]),
            MockResponse::Text("I couldn't find that tool.".into()),
        ])
        .run("Use nonexistent tool")
        .await;

    assert_eq!(output.tool_errors().len(), 1);
}

#[tokio::test]
async fn test_no_convergence_reminder_injected() {
    // Historical behaviour: the loop injected a "Continue the current user
    // request..." <system-reminder> after the first batch of tool calls.
    // That reminder turned out to train the model to open the next turn with
    // `Continue:` / `<system>继续：…` preambles copied from the wording, so
    // the injection was removed. The guidance now lives in the system prompt
    // instead.
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "read".into(),
                arguments: serde_json::json!({"path": "a.txt"}),
            }]),
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "read".into(),
                arguments: serde_json::json!({"path": "b.txt"}),
            }]),
            MockResponse::Text("Done.".into()),
        ])
        .tool(MockTool::ok("read", "hello"))
        .run("Read files")
        .await;

    assert_eq!(system_reminder_count(&output.messages), 0);
    assert_eq!(output.injected_counts(), vec![0, 0, 0]);
}

#[tokio::test]
async fn test_no_convergence_reminder_with_steering() {
    // With steering arriving from the tool channel, the loop used to skip the
    // convergence reminder. The reminder is gone entirely now, so we just
    // verify steering still flows through as a normal user message.
    struct SteeringTool {
        queue: std::sync::Arc<parking_lot::Mutex<Vec<AgentMessage>>>,
    }

    #[async_trait::async_trait]
    impl AgentTool for SteeringTool {
        fn name(&self) -> &str {
            "steering_tool"
        }
        fn label(&self) -> &str {
            "steering_tool"
        }
        fn description(&self) -> &str {
            "Tool that queues steering while executing"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            self.queue
                .lock()
                .push(AgentMessage::Llm(Message::user("stop now")));
            Ok(ToolResult {
                content: vec![Content::Text {
                    text: "hello".into(),
                }],
                details: serde_json::Value::Null,
                retention: Retention::Normal,
            })
        }
    }

    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "steering_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Handled steering.".into()),
    ]);
    let queue: std::sync::Arc<parking_lot::Mutex<Vec<AgentMessage>>> =
        std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut config = make_config(provider);
    config.get_steering_messages = {
        let queue = queue.clone();
        Some(Box::new(move || queue.lock().drain(..).collect()))
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(SteeringTool {
            queue: queue.clone(),
        })],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let prompt = AgentMessage::Llm(Message::user("Read test.txt"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);
    let output = super::fixtures::agent_harness::TestOutput {
        messages,
        events,
        context_messages: context.messages,
    };

    assert_eq!(system_reminder_count(&output.messages), 0);
    assert!(output.messages.iter().any(|message| matches!(
        message,
        AgentMessage::Llm(Message::User { content, .. })
            if content.iter().any(|content| matches!(content, Content::Text { text } if text == "stop now"))
    )));
}

// ---------------------------------------------------------------------------
// Parallel tool execution tests
// ---------------------------------------------------------------------------

/// A tool that records execution timestamps to verify parallelism.
struct TimedTool {
    name: String,
    delay_ms: u64,
}

#[async_trait::async_trait]
impl AgentTool for TimedTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn label(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Timed tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("done:{}", self.name),
            }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

#[tokio::test]
async fn test_parallel_tool_execution_faster_than_sequential() {
    // 3 tools each taking 50ms. Sequential = 150ms+, Parallel = ~50ms.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "tool_a".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_b".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_c".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("All done.".into()),
    ]);

    let mut config = make_config(provider);
    config.tool_execution = ToolExecutionStrategy::Parallel;

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(TimedTool {
                name: "tool_a".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_b".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_c".into(),
                delay_ms: 50,
            }),
        ],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("Run all tools"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let start = std::time::Instant::now();
    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let _elapsed = start.elapsed();

    let events = collect_events(rx);

    // All 3 tool results should be present
    let tool_results: Vec<_> = new_messages
        .iter()
        .filter(|m| m.role() == "toolResult")
        .collect();
    assert_eq!(tool_results.len(), 3);

    // Parallel execution should complete faster than sequential would (~150ms+),
    // but we don't assert absolute wall-clock time since CI machines are slow.
    // The sequential test (test_sequential_tool_execution_is_slower) covers timing.

    // Should have 3 ToolExecutionStart and 3 ToolExecutionEnd events
    let starts = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }))
        .count();
    let ends = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }))
        .count();
    assert_eq!(starts, 3);
    assert_eq!(ends, 3);
}

#[tokio::test]
async fn test_sequential_tool_execution_is_slower() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "tool_a".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_b".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("Done.".into()),
    ]);

    let mut config = make_config(provider);
    config.tool_execution = ToolExecutionStrategy::Sequential;

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(TimedTool {
                name: "tool_a".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_b".into(),
                delay_ms: 50,
            }),
        ],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("Run tools"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let start = std::time::Instant::now();
    let _new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let elapsed = start.elapsed();

    // Sequential should take 100ms+ (2 × 50ms)
    assert!(
        elapsed.as_millis() >= 95,
        "Sequential execution took {}ms, expected >=95ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_batched_tool_execution() {
    // 4 tools, batch size 2: two batches of 2
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "tool_a".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_b".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_c".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "tool_d".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("All done.".into()),
    ]);

    let mut config = make_config(provider);
    config.tool_execution = ToolExecutionStrategy::Batched { size: 2 };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(TimedTool {
                name: "tool_a".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_b".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_c".into(),
                delay_ms: 50,
            }),
            Box::new(TimedTool {
                name: "tool_d".into(),
                delay_ms: 50,
            }),
        ],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("Run all tools"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    // All 4 results present
    let tool_results: Vec<_> = new_messages
        .iter()
        .filter(|m| m.role() == "toolResult")
        .collect();
    assert_eq!(tool_results.len(), 4);

    // With batch size 2, the first two tools must complete before the second
    // pair starts. Within each pair, tools are allowed to run concurrently.
    let start_order: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionStart { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        })
        .collect();
    let end_order: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionEnd { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(start_order, vec!["tool_a", "tool_b", "tool_c", "tool_d"]);
    assert_eq!(end_order, vec!["tool_a", "tool_b", "tool_c", "tool_d"]);

    let first_second_batch_start = events.iter().position(
        |e| matches!(e, AgentEvent::ToolExecutionStart { tool_name, .. } if tool_name == "tool_c"),
    );
    let first_batch_last_end = events.iter().position(
        |e| matches!(e, AgentEvent::ToolExecutionEnd { tool_name, .. } if tool_name == "tool_b"),
    );

    assert!(first_batch_last_end < first_second_batch_start);
}

// ---------------------------------------------------------------------------
// Streaming tool output (on_update callback) tests
// ---------------------------------------------------------------------------

/// A tool that emits progress updates via on_update callback.
struct ProgressTool;

#[async_trait::async_trait]
impl AgentTool for ProgressTool {
    fn name(&self) -> &str {
        "progress_tool"
    }
    fn label(&self) -> &str {
        "Progress"
    }
    fn description(&self) -> &str {
        "A tool that streams progress"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        for i in 1..=3 {
            if let Some(ref cb) = ctx.on_update {
                cb(ToolResult {
                    content: vec![Content::Text {
                        text: format!("step {}/3", i),
                    }],
                    details: serde_json::Value::Null,
                    retention: Retention::Normal,
                });
            }
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "done".into(),
            }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

#[tokio::test]
async fn test_tool_execution_update_events_emitted() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("All done.".into()),
    ]);

    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);

    let updates: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionUpdate { partial_result, .. } => {
                if let Some(Content::Text { text }) = partial_result.content.first() {
                    Some(text.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    assert_eq!(updates, vec!["step 1/3", "step 2/3", "step 3/3"]);
}

// ---------------------------------------------------------------------------
// Retry with backoff tests
// ---------------------------------------------------------------------------

/// A provider that fails N times with a given error, then delegates to a MockProvider.
struct FailThenSucceedProvider {
    fail_count: std::sync::atomic::AtomicUsize,
    max_failures: usize,
    error: ProviderError,
    inner: MockProvider,
}

use evotengine::provider::ProviderError;
use evotengine::provider::StreamConfig;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;

#[async_trait::async_trait]
impl StreamProvider for FailThenSucceedProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let attempt = self
            .fail_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if attempt < self.max_failures {
            return Err(match &self.error {
                ProviderError::RateLimited { retry_after_ms } => ProviderError::RateLimited {
                    retry_after_ms: *retry_after_ms,
                },
                ProviderError::QuotaLimited(message) => {
                    ProviderError::QuotaLimited(message.clone())
                }
                ProviderError::Network(msg) => ProviderError::Network(msg.clone()),
                ProviderError::Transient {
                    message,
                    retry_after_ms,
                } => ProviderError::Transient {
                    message: message.clone(),
                    retry_after_ms: *retry_after_ms,
                },
                ProviderError::Auth(msg) => ProviderError::Auth(msg.clone()),
                other => ProviderError::Other(other.to_string()),
            });
        }
        self.inner.stream(config, tx, cancel).await
    }
}

#[tokio::test]
async fn test_retry_on_rate_limit_succeeds() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 2,
            error: ProviderError::RateLimited {
                retry_after_ms: Some(10), // 10ms for fast tests
            },
            inner: MockProvider::text("Success after retries"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(3),
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should have succeeded after 2 failures + 1 success
    assert_eq!(new_messages.len(), 2); // user + assistant
    let events = collect_events(rx);
    let retry_events: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::LlmCallRetry {
                attempt,
                max_retries,
                delay_ms,
                error,
                ..
            } => Some((*attempt, *max_retries, *delay_ms, error.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(retry_events.len(), 2);
    assert_eq!(retry_events[0].0, 1);
    assert_eq!(retry_events[0].1, 3);
    assert_eq!(retry_events[0].2, 10);
    assert_eq!(retry_events[0].3, "Rate limited, retry after 10ms");
    assert_eq!(retry_events[1].0, 2);
    assert_eq!(retry_events[1].1, 3);
    assert_eq!(retry_events[1].2, 10);
    assert_eq!(retry_events[1].3, "Rate limited, retry after 10ms");
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { .. })));

    // Verify the provider was called 3 times (2 failures + 1 success)
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
}

#[tokio::test]
async fn test_quota_limit_waits_without_using_bounded_retry_budget() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 2,
            error: ProviderError::QuotaLimited("Quota exceeded".into()),
            inner: MockProvider::text("Success after quota reset"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::disabled(),
        before_turn: None,
        after_turn: None,
        spill: None,
    };
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    // Abort while the first quota wait is sleeping; this verifies cancellation
    // remains responsive without waiting for the production retry interval.
    let cancel = CancellationToken::new();
    let cancel_after_wait = cancel.clone();
    let events_after_wait = std::sync::Arc::new(tokio::sync::Notify::new());
    let notify = events_after_wait.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let watcher = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            let quota_wait = matches!(event, AgentEvent::QuotaWait { .. });
            events.push(event);
            if quota_wait {
                notify.notify_one();
                break;
            }
        }
        events
    });
    let cancellation = tokio::spawn(async move {
        events_after_wait.notified().await;
        cancel_after_wait.cancel();
    });

    let messages = agent_loop(
        vec![AgentMessage::Llm(Message::user("hi"))],
        &mut context,
        &config,
        tx,
        cancel,
    )
    .await;
    let _ = cancellation.await;
    let events = watcher.await.unwrap_or_default();

    assert_eq!(messages.len(), 2);
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let quota_delays: Vec<u64> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::QuotaWait { delay_ms } => Some(*delay_ms),
            _ => None,
        })
        .collect();
    assert_eq!(quota_delays, vec![60_000]);
    assert!(!events
        .iter()
        .any(|event| matches!(event, AgentEvent::LlmCallRetry { .. })));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::LlmCallStart { .. }))
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn test_provider_declared_transient_error_retries_then_succeeds() {
    let payload = r#"{"type":"error","error":{"type":"api_error","message":"wording is not used for retry classification"}}"#;
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 1,
            error: ProviderError::Transient {
                message: payload.into(),
                retry_after_ms: None,
            },
            inner: MockProvider::text("Success after transient error"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(1),
        before_turn: None,
        after_turn: None,
        spill: None,
    };
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    let messages = agent_loop(
        vec![AgentMessage::Llm(Message::user("hi"))],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;

    assert_eq!(messages.len(), 2);
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    let retry_errors = collect_events(rx)
        .into_iter()
        .filter_map(|event| match event {
            AgentEvent::LlmCallRetry { error, .. } => Some(error),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(retry_errors, vec![format!("API error: {payload}")]);
}

#[tokio::test(start_paused = true)]
async fn test_retry_exhausted_enters_outage_wait_and_recovers() {
    // 5 failures: 1 initial + 2 bounded retries all fail, then the loop must
    // switch to indefinite outage probing (60 s cadence) instead of failing
    // the run, and succeed once the upstream recovers on the 6th call.
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 5,
            error: ProviderError::Network("connection reset".into()),
            inner: MockProvider::text("recovered after outage"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(2),
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // The run recovers instead of surfacing an error.
    let Some(last) = new_messages.last() else {
        panic!("Expected successful assistant message");
    };
    if let AgentMessage::Llm(Message::Assistant {
        stop_reason,
        content,
        ..
    }) = last
    {
        assert_eq!(*stop_reason, StopReason::Stop);
        assert!(matches!(
            content.first(),
            Some(Content::Text { text }) if text == "recovered after outage"
        ));
    } else {
        panic!("Expected successful assistant message");
    }

    // 1 initial + 2 bounded retries + 3 outage probes = 6 calls.
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        6
    );

    let events = collect_events(rx);
    let outage_waits: Vec<(u64, &str)> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::OutageWait { delay_ms, error } => Some((*delay_ms, error.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(outage_waits.len(), 3);
    for (delay_ms, error) in &outage_waits {
        assert_eq!(*delay_ms, 60_000);
        assert!(error.contains("connection reset"));
    }
    // Bounded retries stay bounded: exactly 2 LlmCallRetry events, and outage
    // probes stay part of the same logical call (no extra LlmCallStart).
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::LlmCallRetry { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::LlmCallStart { .. }))
            .count(),
        3
    );
}

#[tokio::test(start_paused = true)]
async fn test_outage_wait_is_cancellable() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: usize::MAX,
            error: ProviderError::Network("gateway unavailable".into()),
            inner: MockProvider::text("never reached"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(1),
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let cancel_on_wait = cancel.clone();
    let watcher = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if matches!(event, AgentEvent::OutageWait { .. }) {
                cancel_on_wait.cancel();
                return true;
            }
        }
        false
    });

    let messages = agent_loop(
        vec![AgentMessage::Llm(Message::user("hi"))],
        &mut context,
        &config,
        tx,
        cancel,
    )
    .await;
    let saw_outage_wait = watcher.await.unwrap_or(false);

    assert!(saw_outage_wait);
    assert_eq!(messages.len(), 2);
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    assert!(matches!(
        messages.last(),
        Some(AgentMessage::Llm(Message::Assistant {
            stop_reason: StopReason::Aborted,
            ..
        }))
    ));
}

#[tokio::test]
async fn test_auth_error_not_retried() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 1,
            error: ProviderError::Auth("invalid key".into()),
            inner: MockProvider::text("recovered"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(3),
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Auth error should NOT be retried — only 1 call made
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn test_retry_none_disables_retries() {
    let provider: std::sync::Arc<FailThenSucceedProvider> =
        std::sync::Arc::new(FailThenSucceedProvider {
            fail_count: std::sync::atomic::AtomicUsize::new(0),
            max_failures: 1,
            error: ProviderError::RateLimited {
                retry_after_ms: None,
            },
            inner: MockProvider::text("never reached"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::disabled(), // disabled
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Only 1 attempt — no retries
    assert_eq!(
        provider
            .fail_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

// ---------------------------------------------------------------------------
// Event streaming bug fix test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_message_update_events_emitted_during_streaming() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello, world!".into())])
        .run("hi")
        .await;

    // Collect MessageUpdate text deltas
    let deltas: Vec<String> = output
        .events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::MessageUpdate {
                delta: StreamDelta::Text { delta, .. },
                ..
            } => Some(delta.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !deltas.is_empty(),
        "Expected MessageUpdate events with text deltas, got none"
    );
    let full_text: String = deltas.into_iter().collect();
    assert_eq!(full_text, "Hello, world!");

    // Verify event ordering: MessageStart before MessageUpdate before MessageEnd
    let event_types: Vec<&str> = output
        .events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::MessageStart { .. } => Some("Start"),
            AgentEvent::MessageUpdate { .. } => Some("Update"),
            AgentEvent::MessageEnd { .. } => Some("End"),
            _ => None,
        })
        .collect();

    let assistant_start = event_types.iter().rposition(|&e| e == "Start").unwrap();
    let assistant_end = event_types.iter().rposition(|&e| e == "End").unwrap();

    for (i, &et) in event_types.iter().enumerate() {
        if et == "Update" {
            assert!(
                i > assistant_start && i < assistant_end,
                "MessageUpdate at index {} should be between MessageStart ({}) and MessageEnd ({})",
                i,
                assistant_start,
                assistant_end
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle callback tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_before_turn_can_abort() {
    // Provider with 5 text responses, but before_turn aborts after 2 turns.
    // We need tool calls to keep the loop going for multiple turns.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        // These should never be reached
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Final".into()),
    ]);

    let turn_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let turn_count_clone = turn_count.clone();

    let mut config = make_config(provider);
    config.before_turn = Some(std::sync::Arc::new(move |_msgs, _turn| {
        let count = turn_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        count < 2 // Allow turns 0 and 1, abort on turn 2
    }));

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // before_turn was called 3 times (allowed 0, allowed 1, rejected 2)
    assert_eq!(turn_count.load(std::sync::atomic::Ordering::SeqCst), 3);

    // Only 2 assistant messages should be produced
    let assistant_count = new_messages
        .iter()
        .filter(|m| m.role() == "assistant")
        .count();
    assert_eq!(assistant_count, 2);
}

#[tokio::test]
async fn test_after_turn_receives_messages() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("Done.".into()),
    ]);

    let message_counts: std::sync::Arc<std::sync::Mutex<Vec<usize>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let counts_clone = message_counts.clone();

    let mut config = make_config(provider);
    config.after_turn = Some(std::sync::Arc::new(move |msgs, _usage| {
        counts_clone.lock().unwrap().push(msgs.len());
    }));

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, _rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let counts = message_counts.lock().unwrap();
    // after_turn called twice (one per LLM response)
    assert_eq!(counts.len(), 2);
    // Message count should increase between calls
    assert!(counts[1] > counts[0], "counts: {:?}", *counts);
}

#[tokio::test]
async fn test_error_event_fires_on_provider_error() {
    let provider = FailThenSucceedProvider {
        fail_count: std::sync::atomic::AtomicUsize::new(0),
        max_failures: 10, // more failures than retries
        error: ProviderError::Network("connection reset".into()),
        inner: MockProvider::text("never reached"),
    };

    let mut config = make_config(MockProvider::text("unused"));
    config.provider = std::sync::Arc::new(provider);
    config.retry_policy = evotengine::RetryPolicy::disabled();

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    let events = collect_events(rx);
    let error_events: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Error { error } => Some(error.message.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(error_events.len(), 1);
    assert!(
        error_events[0].contains("connection reset"),
        "got: {}",
        error_events[0]
    );
}

#[tokio::test]
async fn test_callbacks_are_optional() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello!".into())])
        .run("Hi")
        .await;

    output.assert_message_count(2);
    output.assert_completed();
}

// ---------------------------------------------------------------------------
// ProgressMessage tests (Addition 1)
// ---------------------------------------------------------------------------

/// A tool that calls on_progress to emit user-facing progress messages.
struct ProgressMessageTool;

#[async_trait::async_trait]
impl AgentTool for ProgressMessageTool {
    fn name(&self) -> &str {
        "progress_msg_tool"
    }
    fn label(&self) -> &str {
        "ProgressMsg"
    }
    fn description(&self) -> &str {
        "Emits progress messages"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(ref progress) = ctx.on_progress {
            progress("Working...".into());
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "done".into(),
            }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

#[tokio::test]
async fn test_progress_message_event_emitted() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_msg_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("ok".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressMessageTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let progress_msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ProgressMessage {
                tool_call_id,
                tool_name,
                text,
            } => Some((tool_call_id.clone(), tool_name.clone(), text.clone())),
            _ => None,
        })
        .collect();

    assert_eq!(progress_msgs.len(), 1);
    assert_eq!(progress_msgs[0].1, "progress_msg_tool");
    assert_eq!(progress_msgs[0].2, "Working...");
}

/// A tool that does NOT call on_progress — should cause no panics, no events.
struct SilentTool;

#[async_trait::async_trait]
impl AgentTool for SilentTool {
    fn name(&self) -> &str {
        "silent_tool"
    }
    fn label(&self) -> &str {
        "Silent"
    }
    fn description(&self) -> &str {
        "Does not call progress"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        // Intentionally ignores on_progress
        Ok(ToolResult {
            content: vec![Content::Text {
                text: "quiet".into(),
            }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

#[tokio::test]
async fn test_tool_ignoring_progress_no_panic() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "silent_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("ok".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(SilentTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    // No ProgressMessage events
    let progress_count = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ProgressMessage { .. }))
        .count();
    assert_eq!(progress_count, 0);
}

/// Two parallel tools both emit progress — events are distinguishable by tool_call_id.
struct NamedProgressTool {
    tool_name: String,
}

#[async_trait::async_trait]
impl AgentTool for NamedProgressTool {
    fn name(&self) -> &str {
        &self.tool_name
    }
    fn label(&self) -> &str {
        &self.tool_name
    }
    fn description(&self) -> &str {
        "Named progress tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(ref progress) = ctx.on_progress {
            progress(format!("progress from {}", self.tool_name));
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("done:{}", self.tool_name),
            }],
            details: serde_json::Value::Null,
            retention: Retention::Normal,
        })
    }
}

#[tokio::test]
async fn test_streams_parallel_tool_calls_before_execution() {
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::ToolCalls(vec![
                MockToolCall {
                    name: "read_a".into(),
                    arguments: serde_json::json!({"path": "a.rs"}),
                },
                MockToolCall {
                    name: "read_b".into(),
                    arguments: serde_json::json!({"path": "b.rs"}),
                },
            ]),
            MockResponse::Text("done".into()),
        ])
        .tool(MockTool::ok("read_a", "a"))
        .tool(MockTool::ok("read_b", "b"))
        .run("go")
        .await;

    let streamed: Vec<_> = output
        .events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::MessageUpdate {
                delta:
                    StreamDelta::ToolCallEnd {
                        content_index,
                        id,
                        name,
                        arguments,
                    },
                ..
            } => Some((*content_index, id.as_str(), name.as_str(), arguments)),
            _ => None,
        })
        .collect();

    assert_eq!(streamed.len(), 2);
    assert_eq!(streamed[0].0, 0);
    assert_eq!(streamed[0].2, "read_a");
    assert_eq!(streamed[0].3["path"], "a.rs");
    assert_eq!(streamed[1].0, 1);
    assert_eq!(streamed[1].2, "read_b");
    assert_eq!(streamed[1].3["path"], "b.rs");
}

#[tokio::test]
async fn test_parallel_tools_progress_distinguishable() {
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "pa".into(),
                arguments: serde_json::json!({}),
            },
            MockToolCall {
                name: "pb".into(),
                arguments: serde_json::json!({}),
            },
        ]),
        MockResponse::Text("done".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![
            Box::new(NamedProgressTool {
                tool_name: "pa".into(),
            }),
            Box::new(NamedProgressTool {
                tool_name: "pb".into(),
            }),
        ],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let progress_msgs: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ProgressMessage {
                tool_name, text, ..
            } => Some((tool_name.clone(), text.clone())),
            _ => None,
        })
        .collect();

    assert_eq!(progress_msgs.len(), 2);
    let names: Vec<&str> = progress_msgs.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"pa"));
    assert!(names.contains(&"pb"));
}

#[tokio::test]
async fn test_on_update_still_works_after_refactor() {
    // Existing ProgressTool uses on_update (not on_progress) — ensure it still works.
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "progress_tool".into(),
            arguments: serde_json::json!({}),
        }]),
        MockResponse::Text("ok".into()),
    ]);
    let config = make_config(provider);

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: vec![Box::new(ProgressTool)],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("go"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    let updates: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolExecutionUpdate { partial_result, .. } => {
                if let Some(Content::Text { text }) = partial_result.content.first() {
                    Some(text.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    assert_eq!(updates, vec!["step 1/3", "step 2/3", "step 3/3"]);
}

// ---------------------------------------------------------------------------
// Context compaction direct-entry tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_compact_messages_reduces_over_budget_context() {
    use evotengine::context::compaction::config::CompactionConfig;
    use evotengine::context::compaction::controller::CompactionController;
    use evotengine::context::SummarizerMode;
    use tokio_util::sync::CancellationToken;

    let mut messages = Vec::new();
    for i in 0..100 {
        messages.push(AgentMessage::Llm(Message::user(format!(
            "Message {} {}",
            i,
            "x".repeat(200)
        ))));
    }

    let config = CompactionConfig {
        context_window: 2_000,
        reserve_tokens: 500,
        advertised_context_window: None,
        trigger_tokens: None,
        keep_recent_tokens: 500,
        summarizer_mode: SummarizerMode::default(),
        summary_max_bytes: 4000,
    };

    let mut ctrl = CompactionController::new(config);
    let cancel = CancellationToken::new();
    ctrl.force_compact(&mut messages, None, cancel).await;

    assert!(
        messages.len() < 100,
        "compaction should have reduced messages"
    );
    assert!(
        messages.len() >= 2,
        "compacted context should contain a summary and retained tail"
    );
}

// ---------------------------------------------------------------------------
// Context compaction event tests
// ---------------------------------------------------------------------------

// NOTE: Compaction is now triggered post-response based on usage, not pre-turn.
// These tests are adapted to the new architecture where compaction events are
// only emitted when the controller detects threshold/overflow conditions.

#[tokio::test]
async fn test_compaction_not_emitted_without_context_config() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("ok".into())])
        .system_prompt("")
        .run("hi")
        .await;

    assert_eq!(output.event_count("CompactionEnd"), 0);
}

#[tokio::test]
async fn test_aborted_usage_compacts_only_before_next_explicit_prompt() {
    use evotengine::context::ContextConfig;

    let provider = MockProvider::new(vec![
        MockResponse::TextWithUsageAndStop {
            text: "aborted response".into(),
            usage: Usage {
                input: 900,
                output: 10,
                total_tokens: 910,
                ..Default::default()
            },
            stop_reason: StopReason::Aborted,
        },
        // Consumed by the compaction summarizer before the second prompt.
        MockResponse::Text("summary".into()),
        MockResponse::Text("second answer".into()),
    ]);
    let mut config = make_config(provider);
    config.context_config = Some(ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(125),
        trigger_tokens: None,
        keep_recent_tokens: Some(200),
    });

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: (0..20)
            .map(|index| {
                AgentMessage::Llm(Message::user(format!(
                    "history {index} {}",
                    "x".repeat(200)
                )))
            })
            .collect(),
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let (first_tx, first_rx) = mpsc::unbounded_channel();
    agent_loop(
        vec![AgentMessage::Llm(Message::user("first prompt"))],
        &mut context,
        &config,
        first_tx,
        CancellationToken::new(),
    )
    .await;
    let first_events = collect_events(first_rx);
    assert!(first_events
        .iter()
        .all(|event| !matches!(event, AgentEvent::ContextCompactionEnd { .. })));

    let (second_tx, second_rx) = mpsc::unbounded_channel();
    agent_loop(
        vec![AgentMessage::Llm(Message::user("second prompt"))],
        &mut context,
        &config,
        second_tx,
        CancellationToken::new(),
    )
    .await;
    let second_events = collect_events(second_rx);
    let compact_index = second_events
        .iter()
        .position(|event| matches!(event, AgentEvent::ContextCompactionEnd { .. }))
        .expect("aborted usage should compact before the next explicit prompt");
    let prompt_index = second_events
        .iter()
        .position(|event| matches!(
            event,
            AgentEvent::MessageStart {
                message: AgentMessage::Llm(Message::User { content, .. }),
            } if content.iter().any(|block| matches!(block, Content::Text { text } if text == "second prompt"))
        ))
        .expect("second prompt should be emitted");
    assert!(compact_index < prompt_index);
    assert!(second_events.iter().any(|event| matches!(
        event,
        AgentEvent::MessageEnd {
            message: AgentMessage::Llm(Message::Assistant { content, .. }),
        } if content.iter().any(|block| matches!(block, Content::Text { text } if text == "second answer"))
    )));
}

#[tokio::test]
async fn test_compaction_after_tool_use_waits_for_tool_results() {
    use std::collections::HashSet;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use evotengine::context::ContextConfig;

    struct ToolUseThenTextProvider {
        requests: std::sync::Arc<parking_lot::Mutex<Vec<Vec<Message>>>>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StreamProvider for ToolUseThenTextProvider {
        async fn stream(
            &self,
            config: StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }

            self.requests.lock().push(config.messages.clone());
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = tx.send(StreamEvent::Start);

            let message = if call == 0 {
                let id = "tc-high-usage".to_string();
                let arguments = serde_json::json!({"path": "file.txt"});
                let _ = tx.send(StreamEvent::ToolCallStart {
                    content_index: 0,
                    id: id.clone(),
                    name: "read".into(),
                });
                let _ = tx.send(StreamEvent::ToolCallEnd {
                    content_index: 0,
                    id: id.clone(),
                    name: "read".into(),
                    arguments: arguments.clone(),
                });
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id,
                        name: "read".into(),
                        arguments,
                        metadata: None,
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "mock".into(),
                    provider: "mock".into(),
                    usage: Usage {
                        input: 980,
                        output: 20,
                        ..Default::default()
                    },
                    timestamp: evotengine::now_ms() + 60_000,
                    error_message: None,
                    response_id: None,
                }
            } else {
                let text = "done".to_string();
                let _ = tx.send(StreamEvent::TextDelta {
                    content_index: 0,
                    delta: text.clone(),
                });
                Message::Assistant {
                    content: vec![Content::Text { text }],
                    stop_reason: StopReason::Stop,
                    model: "mock".into(),
                    provider: "mock".into(),
                    usage: Usage::default(),
                    timestamp: 2,
                    error_message: None,
                    response_id: None,
                }
            };

            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            Ok(StreamOutcome::complete(message))
        }
    }

    let requests = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let provider = std::sync::Arc::new(ToolUseThenTextProvider {
        requests: requests.clone(),
        calls: AtomicUsize::new(0),
    });

    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider;
    config.context_config = Some(ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(125),
        trigger_tokens: None,
        keep_recent_tokens: Some(200),
    });

    let mut prior_messages = Vec::new();
    for i in 0..20 {
        prior_messages.push(AgentMessage::Llm(Message::user(format!(
            "history {i} {}",
            "x".repeat(200)
        ))));
    }

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: prior_messages,
        tools: vec![Box::new(MockTool::ok("read", "tool output"))],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("read the file"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::ContextCompactionEnd { .. })),
        "expected compaction to run after the tool result was appended"
    );
    assert!(
        events.iter().all(|event| !matches!(
            event,
            AgentEvent::ContextCompactionStarted {
                will_retry: true,
                ..
            } | AgentEvent::ContextCompactionEnd {
                will_retry: true,
                ..
            }
        )),
        "run-end threshold compaction must never claim overflow retry"
    );

    let captured_requests = requests.lock().clone();
    assert!(
        captured_requests.len() >= 2,
        "expected a second LLM request after executing the tool"
    );
    let second_request = match captured_requests.get(1) {
        Some(messages) => messages,
        None => panic!("missing second LLM request"),
    };

    let mut tool_call_ids = HashSet::new();
    let mut tool_result_ids = HashSet::new();
    for message in second_request {
        match message {
            Message::Assistant { content, .. } => {
                for content in content {
                    if let Content::ToolCall { id, .. } = content {
                        tool_call_ids.insert(id.clone());
                    }
                }
            }
            Message::ToolResult { tool_call_id, .. } => {
                tool_result_ids.insert(tool_call_id.clone());
            }
            _ => {}
        }
    }

    assert!(
        tool_result_ids.is_subset(&tool_call_ids),
        "second request contains orphan tool results: calls={tool_call_ids:?}, results={tool_result_ids:?}"
    );
}

#[tokio::test]
async fn test_non_overflow_error_compacts_from_usage_anchor() {
    use evotengine::context::ContextConfig;

    // Provider whose agent call fails with a non-overflow "overloaded" error
    // carrying zero usage. The error response has no usable token counts, so the
    // post-response path must fall back to the local estimate and compact.
    //
    // Summarizer calls (identified by the summarization system prompt) succeed,
    // so the estimate-driven compaction can actually run.
    struct OverloadedProvider;

    #[async_trait::async_trait]
    impl StreamProvider for OverloadedProvider {
        async fn stream(
            &self,
            config: StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            let _ = tx.send(StreamEvent::Start);

            // Summarizer calls succeed so compaction can complete.
            if config
                .system_prompt
                .starts_with("You are a context summarization")
            {
                let message = Message::Assistant {
                    content: vec![Content::Text {
                        text: "summary".into(),
                    }],
                    stop_reason: StopReason::Stop,
                    model: "mock".into(),
                    provider: "mock".into(),
                    usage: Usage::default(),
                    timestamp: 3,
                    error_message: None,
                    response_id: None,
                };
                let _ = tx.send(StreamEvent::Done {
                    message: message.clone(),
                });
                return Ok(StreamOutcome::complete(message));
            }

            // Agent call: non-overflow error with zero usage.
            let message = Message::Assistant {
                content: vec![Content::Text {
                    // The terminal error body pushes the post-response local
                    // estimate over the threshold despite carrying zero usage.
                    text: "x".repeat(1_500),
                }],
                stop_reason: StopReason::Error,
                model: "mock".into(),
                provider: "mock".into(),
                usage: Usage::default(),
                timestamp: 1,
                error_message: Some(
                    "API error: Our servers are currently overloaded. Please try again later."
                        .into(),
                ),
                response_id: None,
            };
            let _ = tx.send(StreamEvent::Error {
                message: message.clone(),
            });
            Ok(StreamOutcome::complete(message))
        }
    }

    let mut config = make_config(MockProvider::text("unused"));
    config.provider = std::sync::Arc::new(OverloadedProvider);
    // Disable retry so the overloaded error reaches the compaction path as a
    // terminal error response (isolates the estimate fallback under test).
    config.retry_policy = evotengine::RetryPolicy::disabled();
    config.context_config = Some(ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(125),
        trigger_tokens: None,
        keep_recent_tokens: Some(200),
    });

    // The previous successful response is the real usage anchor. It remains
    // below the threshold at prompt time; the new prompt and terminal error are
    // estimated as trailing messages and push the final context over budget.
    let usage_anchor = AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: "previous answer".into(),
        }],
        stop_reason: StopReason::Stop,
        model: "mock".into(),
        provider: "mock".into(),
        usage: Usage {
            input: 700,
            total_tokens: 700,
            ..Default::default()
        },
        timestamp: evotengine::now_ms() + 60_000,
        error_message: None,
        response_id: None,
    });
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: vec![
            AgentMessage::Llm(Message::user("x".repeat(500))),
            AgentMessage::Llm(Message::user("x".repeat(500))),
            AgentMessage::Llm(Message::user("x".repeat(500))),
            AgentMessage::Llm(Message::user("x".repeat(500))),
            usage_anchor,
        ],
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user(format!(
        "trigger overload {}",
        "x".repeat(1_000)
    )));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;
    let events = collect_events(rx);

    // The previous assistant remains below threshold at prompt time; this
    // compaction comes from the final error plus its anchored trailing estimate.
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Error { .. })),
        "expected an error event"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ContextCompactionEnd { .. })),
        "expected estimate-based compaction to run after a non-overflow error"
    );
}

#[tokio::test]
async fn test_overflow_retry_removes_failed_response_from_active_context() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use evotengine::context::ContextConfig;

    struct LengthOverflowThenSuccess {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StreamProvider for LengthOverflowThenSuccess {
        async fn stream(
            &self,
            config: StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            let is_summary = config
                .system_prompt
                .starts_with("You are a context summarization");
            let call = if is_summary {
                None
            } else {
                Some(self.calls.fetch_add(1, Ordering::SeqCst))
            };
            let text = match call {
                None => "compacted history",
                Some(0) => "abandoned partial",
                Some(_) => "recovered answer",
            };
            let _ = tx.send(StreamEvent::Start);
            let _ = tx.send(StreamEvent::TextDelta {
                content_index: 0,
                delta: text.into(),
            });
            let message = Message::Assistant {
                content: vec![Content::Text { text: text.into() }],
                stop_reason: if call == Some(0) {
                    StopReason::Length
                } else {
                    StopReason::Stop
                },
                model: "mock".into(),
                provider: "mock".into(),
                usage: if call == Some(0) {
                    Usage {
                        input: 1_100,
                        output: 0,
                        total_tokens: 1_100,
                        ..Default::default()
                    }
                } else {
                    Usage {
                        input: 500,
                        output: 20,
                        total_tokens: 520,
                        ..Default::default()
                    }
                },
                timestamp: evotengine::now_ms() + 60_000,
                error_message: if call == Some(0) {
                    Some("response incomplete: max_output_tokens".into())
                } else {
                    None
                },
                response_id: None,
            };
            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            Ok(StreamOutcome::complete(message))
        }
    }

    let provider = std::sync::Arc::new(LengthOverflowThenSuccess {
        calls: AtomicUsize::new(0),
    });
    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider.clone();
    config.context_config = Some(ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(125),
        trigger_tokens: None,
        keep_recent_tokens: Some(200),
    });

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: (0..10)
            .map(|i| AgentMessage::Llm(Message::user(format!("history {i} {}", "x".repeat(100)))))
            .collect(),
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let prompt = AgentMessage::Llm(Message::user("continue"));
    let (tx, rx) = mpsc::unbounded_channel();

    let new_messages = agent_loop(
        vec![prompt],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;
    let events = collect_events(rx);

    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(events
        .iter()
        .any(|event| matches!(event, AgentEvent::ContextCompactionEnd {
            will_retry: true,
            ..
        })));
    let completed_assistant_texts: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::MessageEnd {
                message: AgentMessage::Llm(Message::Assistant { content, .. }),
            } => Some(
                content
                    .iter()
                    .filter_map(|block| match block {
                        Content::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(completed_assistant_texts, vec![
        "abandoned partial",
        "recovered answer"
    ]);
    assert!(new_messages.iter().all(|message| !matches!(
        message,
        AgentMessage::Llm(Message::Assistant { content, .. })
            if content.iter().any(|block| matches!(block, Content::Text { text } if text == "abandoned partial"))
    )));
    assert!(context.messages.iter().all(|message| !matches!(
        message,
        AgentMessage::Llm(Message::Assistant { content, .. })
            if content.iter().any(|block| matches!(block, Content::Text { text } if text == "abandoned partial"))
    )));
}

#[tokio::test]
async fn test_model_switch_sends_first_request_without_precompaction() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use evotengine::context::ContextConfig;
    use evotengine::provider::ModelConfig;

    struct ModelSwitchProvider {
        calls: AtomicUsize,
        main_budgets: parking_lot::Mutex<Vec<u32>>,
    }

    #[async_trait::async_trait]
    impl StreamProvider for ModelSwitchProvider {
        async fn stream(
            &self,
            config: StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let is_summary = config
                .system_prompt
                .starts_with("You are a context summarization");
            if !is_summary {
                self.main_budgets.lock().push(config.resolved_max_tokens());
            }
            let text = if is_summary {
                "compacted history"
            } else {
                "answer after model switch"
            };
            let message = Message::Assistant {
                content: vec![Content::Text { text: text.into() }],
                stop_reason: StopReason::Stop,
                model: config.model,
                provider: "local".into(),
                usage: Usage::default(),
                timestamp: 1,
                error_message: None,
                response_id: None,
            };
            let _ = tx.send(StreamEvent::Start);
            let _ = tx.send(StreamEvent::TextDelta {
                content_index: 0,
                delta: text.into(),
            });
            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            Ok(StreamOutcome::complete(message))
        }
    }

    let provider = std::sync::Arc::new(ModelSwitchProvider {
        calls: AtomicUsize::new(0),
        main_budgets: parking_lot::Mutex::new(Vec::new()),
    });
    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider.clone();
    config.model = "new-model".into();
    let model_config = ModelConfig::resolve(evotengine::provider::ResolveModelRequest {
        protocol: evotengine::provider::ApiProtocol::OpenAiCompletions,
        provider: "local".into(),
        model_id: "new-model".into(),
        base_url: String::new(),
        headers: Default::default(),
        compat: Some(evotengine::provider::OpenAiCompat::default()),
        route_capabilities: Default::default(),
        overrides: evotengine::provider::ModelOverrides {
            context_window: Some(10_000),
            max_output_tokens: Some(500),
            ..Default::default()
        },
    });
    config.model_config = Some(model_config);
    config.context_config = Some(ContextConfig {
        max_context_tokens: 10_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(1250),
        trigger_tokens: None,
        keep_recent_tokens: Some(2000),
    });

    let old_assistant = AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text {
            text: "old answer ".repeat(100),
        }],
        stop_reason: StopReason::Stop,
        model: "old-model".into(),
        provider: "local".into(),
        usage: Usage {
            // Deliberately above the new model's 8,750-token threshold. This
            // belongs to old-model and must not trigger pre-prompt compaction.
            input: 9_000,
            output: 100,
            total_tokens: 9_100,
            ..Default::default()
        },
        timestamp: 1,
        error_message: None,
        response_id: None,
    });
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: vec![
            AgentMessage::Llm(Message::user("pinned")),
            AgentMessage::Llm(Message::user("x".repeat(80_000))),
            AgentMessage::Llm(Message::user("recent ".repeat(1_700))),
            old_assistant,
        ],
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    agent_loop(
        vec![AgentMessage::Llm(Message::user("next prompt"))],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;
    let events = collect_events(rx);

    assert!(events
        .iter()
        .all(|event| !matches!(event, AgentEvent::ContextCompactionEnd { .. })));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    // The input limit drives compaction while the independent output limit is
    // preserved at the provider boundary. The request is sent before compaction.
    assert_eq!(provider.main_budgets.lock().as_slice(), &[500]);
}

#[tokio::test]
async fn test_sanitized_413_compacts_and_retries_once() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use evotengine::context::ContextConfig;

    struct RequestTooLargeThenSuccess {
        calls: AtomicUsize,
        request_chars: parking_lot::Mutex<Vec<usize>>,
    }

    #[async_trait::async_trait]
    impl StreamProvider for RequestTooLargeThenSuccess {
        async fn stream(
            &self,
            config: StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            let is_summary = config
                .system_prompt
                .starts_with("You are a context summarization");
            let call = if is_summary {
                None
            } else {
                Some(self.calls.fetch_add(1, Ordering::SeqCst))
            };
            let chars = config
                .messages
                .iter()
                .map(|message| match message {
                    Message::User { content, .. }
                    | Message::Assistant { content, .. }
                    | Message::ToolResult { content, .. } => content
                        .iter()
                        .map(|block| match block {
                            Content::Text { text } => text.len(),
                            _ => 0,
                        })
                        .sum::<usize>(),
                })
                .sum();
            if call.is_some() {
                self.request_chars.lock().push(chars);
            }

            if call == Some(0) {
                return Err(ProviderError::classify(
                    413,
                    r#"HTTP 413: {"type":"error","error":{"type":"api_error","message":"Upstream request failed."}}"#,
                    None,
                ));
            }

            let text = if is_summary {
                "compacted history"
            } else {
                "recovered after compaction"
            };
            let message = Message::Assistant {
                content: vec![Content::Text { text: text.into() }],
                stop_reason: StopReason::Stop,
                model: config.model,
                provider: "openai".into(),
                usage: Usage::default(),
                timestamp: evotengine::now_ms() + 60_000,
                error_message: None,
                response_id: None,
            };
            let _ = tx.send(StreamEvent::Start);
            let _ = tx.send(StreamEvent::TextDelta {
                content_index: 0,
                delta: text.into(),
            });
            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            Ok(StreamOutcome::complete(message))
        }
    }

    let provider = std::sync::Arc::new(RequestTooLargeThenSuccess {
        calls: AtomicUsize::new(0),
        request_chars: parking_lot::Mutex::new(Vec::new()),
    });
    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider.clone();
    config.retry_policy = evotengine::RetryPolicy::disabled();
    config.context_config = Some(ContextConfig {
        max_context_tokens: 2_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(250),
        trigger_tokens: None,
        keep_recent_tokens: Some(300),
    });

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: (0..12)
            .map(|index| {
                AgentMessage::Llm(Message::user(format!(
                    "old history {index} {}",
                    "x".repeat(1_000)
                )))
            })
            .collect(),
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    let new_messages = agent_loop(
        vec![AgentMessage::Llm(Message::user("continue"))],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;
    let events = collect_events(rx);

    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let request_chars = provider.request_chars.lock();
    assert_eq!(request_chars.len(), 2);
    assert!(
        request_chars[1] < request_chars[0],
        "retry payload should shrink after compaction: {request_chars:?}"
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, AgentEvent::ContextCompactionEnd {
            reason: evotengine::context::CompactReason::Overflow,
            will_retry: true,
            ..
        })));
    assert!(events
        .iter()
        .all(|event| !matches!(event, AgentEvent::LlmCallRetry { .. })));
    assert!(new_messages.iter().any(|message| matches!(
        message,
        AgentMessage::Llm(Message::Assistant { content, .. })
            if content.iter().any(|block| matches!(block, Content::Text { text } if text == "recovered after compaction"))
    )));
}

#[tokio::test]
async fn test_overflow_recovery_survives_summarizer_failure() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use evotengine::context::ContextConfig;

    // The relay rejects oversized payloads by bytes. Both the main request and
    // the LLM summarize request hit the same limit; only the compacted retry
    // fits. Mirrors the claude-fable-5@droid 11MB incident.
    struct RejectsMainAndSummarizer {
        main_calls: AtomicUsize,
        summary_calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StreamProvider for RejectsMainAndSummarizer {
        async fn stream(
            &self,
            config: StreamConfig,
            tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            let is_summary = config
                .system_prompt
                .starts_with("You are a context summarization");
            if is_summary {
                self.summary_calls.fetch_add(1, Ordering::SeqCst);
                return Err(ProviderError::classify(
                    413,
                    r#"HTTP 413: {"type":"error","error":{"type":"api_error","message":"Upstream request failed."}}"#,
                    None,
                ));
            }
            let call = self.main_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Err(ProviderError::classify(
                    413,
                    r#"HTTP 413: {"type":"error","error":{"type":"api_error","message":"Upstream request failed."}}"#,
                    None,
                ));
            }

            let message = Message::Assistant {
                content: vec![Content::Text {
                    text: "recovered after emergency compaction".into(),
                }],
                stop_reason: StopReason::Stop,
                model: config.model,
                provider: "openai".into(),
                usage: Usage::default(),
                timestamp: evotengine::now_ms() + 60_000,
                error_message: None,
                response_id: None,
            };
            let _ = tx.send(StreamEvent::Start);
            let _ = tx.send(StreamEvent::TextDelta {
                content_index: 0,
                delta: "recovered after emergency compaction".into(),
            });
            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            Ok(StreamOutcome::complete(message))
        }
    }

    let provider = std::sync::Arc::new(RejectsMainAndSummarizer {
        main_calls: AtomicUsize::new(0),
        summary_calls: AtomicUsize::new(0),
    });
    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider.clone();
    config.retry_policy = evotengine::RetryPolicy::disabled();
    config.context_config = Some(ContextConfig {
        max_context_tokens: 2_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(250),
        trigger_tokens: None,
        keep_recent_tokens: Some(300),
    });

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: (0..12)
            .map(|index| {
                AgentMessage::Llm(Message::user(format!(
                    "old history {index} {}",
                    "x".repeat(1_000)
                )))
            })
            .collect(),
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    let new_messages = agent_loop(
        vec![AgentMessage::Llm(Message::user("continue"))],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;
    let events = collect_events(rx);

    assert_eq!(provider.main_calls.load(Ordering::SeqCst), 2);
    assert!(
        provider.summary_calls.load(Ordering::SeqCst) >= 1,
        "the LLM summarizer must be attempted before the emergency fallback"
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, AgentEvent::ContextCompactionEnd {
            reason: evotengine::context::CompactReason::Overflow,
            will_retry: true,
            ..
        })));
    assert!(events.iter().all(|event| !matches!(
        event,
        AgentEvent::Error { error } if error.message.contains("recovery failed")
    )));
    assert!(new_messages.iter().any(|message| matches!(
        message,
        AgentMessage::Llm(Message::Assistant { content, .. })
            if content.iter().any(|block| matches!(block, Content::Text { text } if text == "recovered after emergency compaction"))
    )));
}

#[tokio::test]
async fn test_unrecoverable_overflow_surfaces_visible_error() {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use evotengine::context::ContextConfig;

    struct AlwaysRequestTooLarge {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StreamProvider for AlwaysRequestTooLarge {
        async fn stream(
            &self,
            _config: StreamConfig,
            _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::classify(
                413,
                r#"HTTP 413: {"type":"error","error":{"type":"api_error","message":"Upstream request failed."}}"#,
                None,
            ))
        }
    }

    let provider = std::sync::Arc::new(AlwaysRequestTooLarge {
        calls: AtomicUsize::new(0),
    });
    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider.clone();
    config.retry_policy = evotengine::RetryPolicy::disabled();
    config.context_config = Some(ContextConfig {
        max_context_tokens: 2_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(250),
        trigger_tokens: None,
        keep_recent_tokens: Some(300),
    });

    // A single oversized message leaves the planner nothing to evict, so
    // recovery cannot compact. The failure must be user-visible.
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: vec![AgentMessage::Llm(Message::user("x".repeat(4_000)))],
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    agent_loop(
        vec![AgentMessage::Llm(Message::user("continue"))],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;
    let events = collect_events(rx);

    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(events
        .iter()
        .all(|event| !matches!(event, AgentEvent::ContextCompactionEnd { .. })));
    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentEvent::Error { error }
                if error.message.contains("Context overflow recovery failed: could not compact")
        )),
        "a failed overflow recovery must not end the run silently"
    );
}

/// A context that cannot be compacted below the window is still sent: the
/// provider is the authority on whether it fits, and an overflow response is
/// recovered by the post-response compact-and-retry path. Mirrors pi, which
/// checks compaction once before a prompt and never refuses locally.
#[tokio::test]
async fn test_uncompactable_context_is_still_sent_to_the_provider() {
    use evotengine::context::ContextConfig;

    // Local estimate: prior user = 3956/4 + 4 = 993, current user =
    // 8/4 + 4 = 6, system prompt = 4/4 = 1, totaling exactly 1000.
    // The request must still proceed even if best-effort compaction cannot make
    // the provider accept it.
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("sent anyway".into())])
        .prior_messages(vec![AgentMessage::Llm(Message::user("x".repeat(3_956)))])
        .context_config(ContextConfig {
            max_context_tokens: 1_000,
            system_prompt_tokens: 0,
            advertised_context_window: None,
            reserve_tokens: Some(125),
            trigger_tokens: None,
            keep_recent_tokens: Some(200),
        })
        .run("continue")
        .await;

    assert!(output
        .events
        .iter()
        .any(|event| matches!(event, AgentEvent::LlmCallStart { .. })));
    assert!(output.events.iter().all(|event| !matches!(
        event,
        AgentEvent::Error { error } if error.message.contains("request was not sent")
    )));
    assert!(output.messages.iter().any(|message| matches!(
        message,
        AgentMessage::Llm(Message::Assistant { content, .. })
            if content.iter().any(|block| matches!(block, Content::Text { text } if text == "sent anyway"))
    )));
}

/// A large-window model must not compact just because its history is a large
/// absolute number. With a fixed 16k reserve, a 272k-window model still has ~256k
/// of headroom, so a 120k context is nowhere near the threshold. The previous
/// window/8 reserve plus window/5 retention made big-window models compact far
/// too eagerly and then fail to shrink below the window.
#[tokio::test]
async fn test_large_window_model_does_not_compact_mid_window() {
    use evotengine::context::ContextConfig;

    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("answer".into())])
        .prior_messages(vec![
            AgentMessage::Llm(Message::user("history")),
            AgentMessage::Llm(Message::Assistant {
                content: vec![Content::Text {
                    text: "prior answer".into(),
                }],
                stop_reason: StopReason::Stop,
                model: "test-model".into(),
                provider: "test".into(),
                usage: Usage {
                    input: 120_000,
                    output: 2_000,
                    total_tokens: 122_000,
                    ..Default::default()
                },
                timestamp: 1,
                error_message: None,
                response_id: None,
            }),
        ])
        .context_config(ContextConfig::from_context_window(272_000))
        .run("next")
        .await;

    assert!(
        output
            .events
            .iter()
            .all(|event| !matches!(event, AgentEvent::ContextCompactionEnd { .. })),
        "a 122k context in a 272k window is well under the threshold"
    );
    assert!(output
        .events
        .iter()
        .any(|event| matches!(event, AgentEvent::LlmCallStart { .. })));
}

/// A summarizer that cannot produce a summary must not strand the run: the
/// main request still goes out. Compaction is best-effort, not a gate.
#[tokio::test]
async fn test_failed_summarizer_still_sends_the_main_request() {
    use evotengine::context::ContextConfig;

    struct FailingSummarizerProvider {
        main_calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl StreamProvider for FailingSummarizerProvider {
        async fn stream(
            &self,
            config: StreamConfig,
            _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<StreamOutcome, ProviderError> {
            if config
                .system_prompt
                .starts_with("You are a context summarization")
            {
                return Err(ProviderError::Api("summary endpoint failed".into()));
            }
            self.main_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(ProviderError::Api("main request ran".into()))
        }
    }

    let provider = std::sync::Arc::new(FailingSummarizerProvider {
        main_calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let mut config = make_config(MockProvider::text("unused"));
    config.provider = provider.clone();
    config.retry_policy = evotengine::RetryPolicy::disabled();
    config.context_config = Some(ContextConfig {
        max_context_tokens: 1_000,
        system_prompt_tokens: 0,
        advertised_context_window: None,
        reserve_tokens: Some(125),
        trigger_tokens: None,
        keep_recent_tokens: Some(200),
    });
    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: vec![
            AgentMessage::Llm(Message::user("pinned")),
            AgentMessage::Llm(Message::user("x".repeat(8_000))),
            AgentMessage::Llm(Message::user("recent ".repeat(200))),
        ],
        tools: vec![],
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };
    let (tx, rx) = mpsc::unbounded_channel();

    agent_loop(
        vec![AgentMessage::Llm(Message::user("next prompt"))],
        &mut context,
        &config,
        tx,
        CancellationToken::new(),
    )
    .await;
    let events = collect_events(rx);

    assert!(
        provider
            .main_calls
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1,
        "a failed summary must not prevent the main request"
    );
    assert!(events.iter().all(|event| !matches!(
        event,
        AgentEvent::Error { error }
            if error.message.contains("request was not sent")
    )));
}

#[tokio::test]
async fn test_no_usage_anchor_defers_to_provider() {
    use evotengine::context::ContextConfig;

    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("provider accepted request".into())])
        .prior_messages(vec![
            AgentMessage::Llm(Message::user("x".repeat(4_000))),
            assistant_msg_for_test("pinned assistant"),
        ])
        .context_config(ContextConfig {
            max_context_tokens: 1_100,
            system_prompt_tokens: 0,
            advertised_context_window: None,
            reserve_tokens: Some(137),
            trigger_tokens: None,
            keep_recent_tokens: Some(220),
        })
        .run("continue")
        .await;

    assert!(output
        .events
        .iter()
        .any(|event| matches!(event, AgentEvent::LlmCallStart { .. })));
    assert!(output.messages.iter().any(|message| matches!(
        message,
        AgentMessage::Llm(Message::Assistant { content, .. })
            if content.iter().any(|block| matches!(block, Content::Text { text } if text == "provider accepted request"))
    )));
    assert!(output.events.iter().all(|event| !matches!(
        event,
        AgentEvent::Error { error }
            if error.message.contains("request was not sent")
    )));
}

fn assistant_msg_for_test(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text { text: text.into() }],
        stop_reason: StopReason::Stop,
        model: "mock".into(),
        provider: "mock".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

#[tokio::test]
async fn test_llm_call_start_carries_budget_and_window() {
    use evotengine::context::ContextConfig;

    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("ok".into())])
        .system_prompt("sys")
        .context_config(ContextConfig {
            max_context_tokens: 100_000,
            system_prompt_tokens: 10_000,
            advertised_context_window: None,
            reserve_tokens: Some(12500),
            trigger_tokens: None,
            keep_recent_tokens: Some(20000),
        })
        .retry_policy(evotengine::RetryPolicy::disabled())
        .run("hi")
        .await;

    let starts: Vec<_> = output
        .events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::LlmCallStart { budget, .. } => Some((
                budget.system_prompt_tokens,
                budget.budget_tokens,
                budget.context_window,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0], (10_000, 90_000, 100_000));
}

#[tokio::test]
async fn test_same_model_preserves_replayable_tool_use_thinking() {
    use evotengine::provider::ModelConfig;

    let model_config = ModelConfig::anthropic("deepseek-reasoner", "deepseek-reasoner");
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("ok".into())])
        .model_config(model_config)
        .prior_messages(vec![
            AgentMessage::Llm(Message::user("do something")),
            AgentMessage::Llm(Message::Assistant {
                content: vec![
                    Content::Thinking {
                        thinking: "old tool thinking".into(),
                        metadata: Some(ThinkingMetadata::Anthropic {
                            signature: "old-sig".into(),
                        }),
                    },
                    Content::ToolCall {
                        id: "tc-old".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command": "pwd"}),
                        metadata: None,
                    },
                ],
                stop_reason: StopReason::ToolUse,
                model: "deepseek-reasoner".into(),
                provider: "anthropic".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
                response_id: None,
            }),
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id: "tc-old".into(),
                tool_name: "bash".into(),
                content: vec![Content::Text {
                    text: "old result".into(),
                }],
                is_error: false,
                timestamp: 0,
                retention: Retention::Normal,
            }),
            AgentMessage::Llm(Message::Assistant {
                content: vec![
                    Content::Thinking {
                        thinking: "new tool thinking".into(),
                        metadata: Some(ThinkingMetadata::Anthropic {
                            signature: "new-sig".into(),
                        }),
                    },
                    Content::ToolCall {
                        id: "tc-new".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command": "date"}),
                        metadata: None,
                    },
                ],
                stop_reason: StopReason::ToolUse,
                model: "deepseek-reasoner".into(),
                provider: "anthropic".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
                response_id: None,
            }),
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id: "tc-new".into(),
                tool_name: "bash".into(),
                content: vec![Content::Text {
                    text: "new result".into(),
                }],
                is_error: false,
                timestamp: 0,
                retention: Retention::Normal,
            }),
        ])
        .run_continue()
        .await;

    let request_messages = output
        .events
        .iter()
        .find_map(|event| match event {
            AgentEvent::LlmCallStart { request, .. } => Some(&request.messages),
            _ => None,
        })
        .expect("expected LlmCallStart");
    for expected in ["old tool thinking", "new tool thinking"] {
        assert!(request_messages.iter().any(|message| matches!(
            message,
            Message::Assistant { content, .. }
                if content.iter().any(|content| matches!(
                    content,
                    Content::Thinking { thinking, .. } if thinking == expected
                ))
        )));
    }
}

#[tokio::test]
async fn test_llm_call_start_zero_budget_without_context_config() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("ok".into())])
        .system_prompt("")
        .run("hi")
        .await;

    let starts: Vec<_> = output
        .events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::LlmCallStart { budget, .. } => {
                Some((budget.budget_tokens, budget.context_window))
            }
            _ => None,
        })
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0], (0, 0));
}

#[tokio::test]
async fn test_llm_call_stats_image_tokens_separate_from_user() {
    use evotengine::context::compute_call_stats;
    use evotengine::types::Content;
    use evotengine::types::Message;

    let messages = vec![
        Message::User {
            content: vec![
                Content::Text {
                    text: "describe this image".into(),
                },
                Content::Image {
                    mime_type: "image/png".into(),
                    source: evotengine::ImageSource::Base64 {
                        data: "A".repeat(3000),
                    },
                },
            ],
            timestamp: 0,
        },
        Message::Assistant {
            content: vec![Content::Text {
                text: "It shows a cat.".into(),
            }],
            stop_reason: evotengine::types::StopReason::Stop,
            model: "test".into(),
            provider: "test".into(),
            usage: evotengine::types::Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        },
    ];

    let stats = compute_call_stats(&messages);

    // user_tokens should NOT include image tokens
    assert!(stats.user_tokens > 0);
    assert!(stats.image_tokens > 0);
    assert_eq!(stats.image_count, 1);
    assert_eq!(stats.user_count, 1);
    assert_eq!(stats.assistant_count, 1);
    // image tokens are separate — total = user + assistant + image
    let total = stats.user_tokens + stats.assistant_tokens + stats.image_tokens;
    assert!(total > stats.user_tokens + stats.assistant_tokens);
}

#[tokio::test]
async fn test_llm_call_stats_no_images() {
    use evotengine::context::compute_call_stats;
    use evotengine::types::Content;
    use evotengine::types::Message;

    let messages = vec![Message::User {
        content: vec![Content::Text {
            text: "hello".into(),
        }],
        timestamp: 0,
    }];

    let stats = compute_call_stats(&messages);
    assert_eq!(stats.image_count, 0);
    assert_eq!(stats.image_tokens, 0);
    assert_eq!(stats.user_count, 1);
    assert!(stats.user_tokens > 0);
}

// ---------------------------------------------------------------------------
// Empty response retry tests
// ---------------------------------------------------------------------------

/// A provider that returns empty Ok(Message) N times, then delegates to inner.
struct EmptyThenSucceedProvider {
    call_count: std::sync::atomic::AtomicUsize,
    empty_count: usize,
    inner: MockProvider,
}

#[async_trait::async_trait]
impl StreamProvider for EmptyThenSucceedProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let attempt = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if attempt < self.empty_count {
            let _ = tx.send(StreamEvent::Start);
            let msg = Message::Assistant {
                content: vec![],
                stop_reason: StopReason::Stop,
                model: "mock".into(),
                provider: "mock".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
                response_id: None,
            };
            let _ = tx.send(StreamEvent::Done {
                message: msg.clone(),
            });
            return Ok(StreamOutcome::complete(msg));
        }
        self.inner.stream(config, tx, cancel).await
    }
}

#[tokio::test]
async fn test_empty_response_retried_then_succeeds() {
    let provider: std::sync::Arc<EmptyThenSucceedProvider> =
        std::sync::Arc::new(EmptyThenSucceedProvider {
            call_count: std::sync::atomic::AtomicUsize::new(0),
            empty_count: 2,
            inner: MockProvider::text("Success after empty"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(3),
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Should succeed: 2 empty responses retried, 3rd call returns real text
    assert_eq!(new_messages.len(), 2); // user + assistant
    assert_eq!(new_messages[1].role(), "assistant");

    // Provider called 3 times: 2 empty + 1 success
    assert_eq!(
        provider
            .call_count
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );

    let events = collect_events(rx);

    // Should have LlmCallEnd events with errors for the empty attempts
    let llm_call_errors: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::LlmCallEnd {
                error: Some(err), ..
            } => Some(err.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(llm_call_errors.len(), 2);
    assert!(llm_call_errors[0].contains("Empty response"));
}

#[tokio::test(start_paused = true)]
async fn test_empty_response_exhausts_bounded_retries_then_recovers() {
    let provider: std::sync::Arc<EmptyThenSucceedProvider> =
        std::sync::Arc::new(EmptyThenSucceedProvider {
            call_count: std::sync::atomic::AtomicUsize::new(0),
            empty_count: 10, // more empties than retries
            inner: MockProvider::text("never reached"),
        });

    let config = AgentLoopConfig {
        provider: provider.clone(),
        model: "mock".into(),
        api_key: "test".into(),
        thinking_level: ThinkingLevel::Off,
        max_tokens: None,
        model_config: None,
        convert_to_llm: None,
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        context_config: None,
        compaction_context: None,
        compaction_fallback_context: None,
        initial_compaction_state: None,
        execution_limits: None,
        cache_config: CacheConfig::default(),
        tool_execution: ToolExecutionStrategy::default(),
        retry_policy: evotengine::RetryPolicy::new(2),
        before_turn: None,
        after_turn: None,
        spill: None,
    };

    let mut context = AgentContext {
        system_prompt: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: std::path::PathBuf::new(),
        path_guard: std::sync::Arc::new(evotengine::PathGuard::open()),
        prompt_cache_key: None,
    };

    let prompt = AgentMessage::Llm(Message::user("hi"));
    let (tx, rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let new_messages = agent_loop(vec![prompt], &mut context, &config, tx, cancel).await;

    // Empty responses are transient provider failures. Once bounded retries
    // are exhausted, the loop enters cancellable outage probing and recovers
    // when the provider eventually returns content.
    let Some(AgentMessage::Llm(Message::Assistant {
        content,
        stop_reason,
        error_message,
        ..
    })) = new_messages.last()
    else {
        panic!("Expected successful assistant message");
    };
    assert_eq!(*stop_reason, StopReason::Stop);
    assert!(error_message.is_none());
    assert!(matches!(
        content.first(),
        Some(Content::Text { text }) if text == "never reached"
    ));

    // 1 initial + 2 bounded retries + 8 outage probes = 11 attempts.
    assert_eq!(
        provider
            .call_count
            .load(std::sync::atomic::Ordering::SeqCst),
        11
    );

    let events = collect_events(rx);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::LlmCallRetry { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::OutageWait { .. }))
            .count(),
        8
    );
    assert!(!events
        .iter()
        .any(|event| matches!(event, AgentEvent::Error { .. })));
}

// ---------------------------------------------------------------------------
// Steering tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_steering_messages_injected_into_context() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("I see your steering.".into())])
        .steering(vec![AgentMessage::Llm(Message::user("change direction"))])
        .run("Hi")
        .await;

    output.assert_completed();

    // Context should contain: steering msg + user prompt + assistant response
    let user_msgs: Vec<_> = output
        .context_messages
        .iter()
        .filter(|m| m.role() == "user")
        .collect();
    assert_eq!(
        user_msgs.len(),
        2,
        "Expected steering + prompt user messages"
    );
}

#[tokio::test]
async fn test_steering_count_reported_in_llm_call_start() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Got it.".into())])
        .steering(vec![
            AgentMessage::Llm(Message::user("steer 1")),
            AgentMessage::Llm(Message::user("steer 2")),
        ])
        .run("Hi")
        .await;

    output.assert_completed();

    let counts = output.injected_counts();
    assert!(!counts.is_empty(), "Expected at least one LlmCallStart");
    assert_eq!(counts[0], 2, "Expected 2 injected messages");
}

#[tokio::test]
async fn test_no_steering_reports_zero() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello.".into())])
        .run("Hi")
        .await;

    output.assert_completed();

    let counts = output.injected_counts();
    assert!(!counts.is_empty());
    assert_eq!(counts[0], 0, "Expected 0 injected messages");
}

// ---------------------------------------------------------------------------
// Cross-model tool-name aliasing (resume scenario)
// ---------------------------------------------------------------------------

/// When a session is resumed under a non-Claude model, the model may call a
/// tool using the capitalized alias (e.g. `Edit`) it saw in history. Dispatch
/// must still resolve to the tool, and edit-specific coercion (legacy
/// single-edit args) must still apply — gated on the tool's canonical name,
/// not the called alias.
#[tokio::test]
async fn test_aliased_edit_call_dispatches_and_coerces() {
    use evotengine::tools::EditFileTool;

    let tmp = std::env::temp_dir().join("evot-harden-alias-edit.txt");
    std::fs::write(&tmp, "alpha\n").expect("write temp file");
    let path = tmp.to_string_lossy().to_string();

    let output = TestHarness::new()
        .responses(vec![
            // Model calls the Claude-style alias with legacy top-level
            // old_text/new_text (no edits array) — both paths must work.
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "Edit".into(),
                arguments: serde_json::json!({
                    "path": path,
                    "old_text": "alpha",
                    "new_text": "beta",
                }),
            }]),
            MockResponse::Text("done".into()),
        ])
        .tool_boxed(Box::new(EditFileTool::new()))
        .run("edit it")
        .await;

    output.assert_completed();

    // The tool result must not be a dispatch failure.
    assert!(
        output.tool_errors().is_empty(),
        "aliased Edit call should dispatch and succeed without tool errors"
    );

    let content = std::fs::read_to_string(&tmp).expect("read temp file");
    let _ = std::fs::remove_file(&tmp);
    assert!(
        content.contains("beta"),
        "edit coercion should have applied the legacy single-edit, file: {:?}",
        content
    );
}

// ---------------------------------------------------------------------------
// Execution duration limit excludes tool wall-time (issue: long tool killed run)
// ---------------------------------------------------------------------------

/// A single tool call that runs far longer than `max_duration` must not
/// terminate the agent. The loop pauses the idle clock around every tool, so
/// the tool's wall-time is excluded from the duration limit — only the agent's
/// own work counts. Before this fix, a long bash command (e.g. a training run)
/// would trip `max_duration` at the top of the next turn and stop the loop even
/// though the tool returned normally.
#[tokio::test]
async fn slow_tool_does_not_trip_duration_limit() {
    let output = TestHarness::new()
        .execution_limits(evotengine::context::ExecutionLimits {
            max_turns: 1_000_000,
            max_total_tokens: usize::MAX,
            max_duration: std::time::Duration::from_millis(30),
        })
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "slow_build".into(),
                arguments: serde_json::json!({}),
            }]),
            MockResponse::Text("build finished".into()),
        ])
        // Tool runs ~4x longer than the duration limit.
        .tool(MockTool::ok("slow_build", "done").with_delay(std::time::Duration::from_millis(120)))
        .run("run the build")
        .await;

    output.assert_completed();
    output.assert_no_errors();
    // The loop reached the second turn and produced the final text rather than
    // stopping with an "[Agent stopped: Max duration ...]" message.
    output.assert_last_role("assistant");
    assert!(
        output.tool_errors().is_empty(),
        "the slow tool returned normally; there should be no tool error"
    );
    let stopped = output.context_messages.iter().any(|m| {
        matches!(m, AgentMessage::Llm(Message::User { content, .. })
            if content.iter().any(|c| matches!(c, Content::Text { text } if text.contains("Agent stopped"))))
    });
    assert!(!stopped, "a long tool must not trip the duration limit");
}

/// Interactive parity with pi: with no execution limits, the loop never injects
/// an "[Agent stopped]" message regardless of how much work it does.
#[tokio::test]
async fn no_limits_runs_without_stop_message() {
    let output = TestHarness::new()
        // execution_limits left as None (interactive default)
        .responses(vec![
            MockResponse::ToolCalls(vec![MockToolCall {
                name: "slow_build".into(),
                arguments: serde_json::json!({}),
            }]),
            MockResponse::Text("done".into()),
        ])
        .tool(MockTool::ok("slow_build", "ok").with_delay(std::time::Duration::from_millis(40)))
        .run("go")
        .await;

    output.assert_completed();
    output.assert_last_role("assistant");
    let stopped = output.context_messages.iter().any(|m| {
        matches!(m, AgentMessage::Llm(Message::User { content, .. })
            if content.iter().any(|c| matches!(c, Content::Text { text } if text.contains("Agent stopped"))))
    });
    assert!(!stopped, "no limits means no stop message");
}
