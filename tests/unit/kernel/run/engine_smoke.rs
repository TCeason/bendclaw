use std::collections::HashSet;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use bendclaw::kernel::run::compaction::Compactor;
use bendclaw::kernel::run::context::Context;
use bendclaw::kernel::run::engine::QueryEngine;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::tools::execution::labels::ExecutionLabels;
use bendclaw::kernel::tools::execution::progressive::ProgressiveToolView;
use bendclaw::kernel::tools::execution::registry::ToolRegistry;
use bendclaw::kernel::tools::execution::ToolStack;
use bendclaw::kernel::tools::execution::ToolStackConfig;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::kernel::tools::ToolRuntime;
use bendclaw::kernel::trace::TraceRecorder;
use bendclaw::kernel::Message;
use bendclaw::storage::dal::trace::repo::SpanRepo;
use bendclaw::storage::dal::trace::repo::TraceRepo;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::mocks::llm::MockTurn;
use tokio_util::sync::CancellationToken;

fn trace() -> TraceRecorder {
    let pool = bendclaw_test_harness::mocks::context::dummy_pool();
    TraceRecorder::with_writer(
        bendclaw::kernel::trace::TraceWriter::noop(),
        Arc::new(TraceRepo::new(pool.clone())),
        Arc::new(SpanRepo::new(pool)),
        "trace-1",
        "run-1",
        "agent-1",
        "session-1",
        "user-1",
    )
}

fn build_engine_with_filter(
    llm: Arc<MockLLMProvider>,
    allowed: Option<HashSet<String>>,
) -> (QueryEngine, tokio::sync::mpsc::Receiver<Event>) {
    let cancel = CancellationToken::new();
    let (tx, rx) = QueryEngine::create_channel();
    let (_inbox_tx, inbox_rx) = QueryEngine::create_inbox();
    let workspace = bendclaw_test_harness::mocks::context::test_workspace(
        std::env::temp_dir().join("bendclaw-engine-smoke"),
    );
    let labels = Arc::new(ExecutionLabels {
        trace_id: "trace-1".to_string(),
        run_id: "run-1".to_string(),
        session_id: "session-1".to_string(),
        agent_id: "agent-1".to_string(),
    });
    let tool_stack = ToolStack::build(ToolStackConfig {
        tool_registry: Arc::new(ToolRegistry::new()),
        skill_executor: Arc::new(bendclaw::kernel::skills::noop::NoopSkillExecutor),
        tool_context: ToolContext {
            user_id: "user-1".into(),
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            run_id: "run-1".into(),
            trace_id: "trace-1".into(),
            workspace,
            is_dispatched: false,
            runtime: ToolRuntime {
                event_tx: None,
                cancel: cancel.clone(),
                tool_call_id: None,
            },
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        },
        labels,
        cancel: cancel.clone(),
        trace: bendclaw::kernel::trace::Trace::new(trace()),
        event_tx: tx.clone(),
        allowed_tool_names: allowed,
    });
    let ctx = Context {
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        session_id: "session-1".into(),
        run_id: "run-1".into(),
        turn: 1,
        trace_id: "trace-1".into(),
        llm: llm.clone(),
        model: "mock".into(),
        temperature: 0.0,
        max_iterations: 5,
        max_context_tokens: 250_000,
        max_duration: Duration::from_secs(30),
        tool_view: ProgressiveToolView::new(Arc::new(vec![])),
        system_prompt: "test".into(),
        messages: vec![Message::user("hello")],
    };
    let compactor = Compactor::new(llm, "mock".into(), cancel.clone());
    let engine = QueryEngine::from_tx(
        ctx,
        tool_stack.lifecycle,
        compactor,
        cancel,
        Arc::new(AtomicU32::new(0)),
        trace(),
        tx,
        inbox_rx,
        None,
    );
    (engine, rx)
}

fn build_engine(llm: Arc<MockLLMProvider>) -> (QueryEngine, tokio::sync::mpsc::Receiver<Event>) {
    build_engine_with_filter(llm, None)
}

#[tokio::test]
async fn engine_no_tool_call_completes_with_end_turn() {
    let llm = Arc::new(MockLLMProvider::with_text("done"));
    let (mut engine, _rx) = build_engine(llm);
    let result = engine.run().await.unwrap();
    assert_eq!(result.stop_reason, Reason::EndTurn);
    assert_eq!(result.iterations, 1);
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn engine_tool_call_then_final_response() {
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "bash".to_string(),
            arguments: r#"{"command":"echo hi"}"#.to_string(),
        },
        MockTurn::Text("final answer".to_string()),
    ]));
    let (mut engine, _rx) = build_engine(llm);
    let result = engine.run().await.unwrap();
    assert_eq!(result.stop_reason, Reason::EndTurn);
    assert!(result.iterations >= 2);
}

#[tokio::test]
async fn engine_filter_blocks_disallowed_tool() {
    let allowed: HashSet<String> = ["read"].iter().map(|s| s.to_string()).collect();
    let llm = Arc::new(MockLLMProvider::new(vec![
        MockTurn::ToolCall {
            name: "bash".to_string(),
            arguments: r#"{"command":"echo hi"}"#.to_string(),
        },
        MockTurn::Text("done".to_string()),
    ]));
    let (mut engine, _rx) = build_engine_with_filter(llm, Some(allowed));
    let result = engine.run().await.unwrap();
    assert_eq!(result.stop_reason, Reason::EndTurn);
    let tool_results: Vec<_> = result
        .messages
        .iter()
        .filter(|m| matches!(m, Message::ToolResult { .. }))
        .collect();
    assert!(
        !tool_results.is_empty(),
        "should have tool result (error) for blocked tool"
    );
}
