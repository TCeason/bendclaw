use bendclaw::kernel::recall::post_run::process_run_events;
use bendclaw::kernel::recall::RecallStore;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::tools::operation::OpType;
use bendclaw::kernel::tools::OperationMeta;

use crate::common::fake_databend::rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn make_tool_start(tool_call_id: &str, name: &str, args: serde_json::Value) -> Event {
    Event::ToolStart {
        tool_call_id: tool_call_id.to_string(),
        name: name.to_string(),
        arguments: args,
    }
}

fn make_tool_end(tool_call_id: &str, name: &str, success: bool, output: &str) -> Event {
    Event::ToolEnd {
        tool_call_id: tool_call_id.to_string(),
        name: name.to_string(),
        success,
        output: output.to_string(),
        operation: OperationMeta {
            op_type: OpType::Execute,
            impact: None,
            timeout_secs: None,
            duration_ms: 100,
            summary: String::new(),
        },
    }
}

#[tokio::test]
async fn creates_knowledge_for_file_write() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = RecallStore::new(fake.pool());
    let events = vec![
        make_tool_start(
            "tc-1",
            "file_write",
            serde_json::json!({"path": "/tmp/test.rs"}),
        ),
        make_tool_end("tc-1", "file_write", true, "ok"),
    ];

    process_run_events(&store, "run-1", "user-1", &events).await;

    let calls = fake.calls();
    assert!(calls.iter().any(|call| {
        match call {
            FakeDatabendCall::Query { sql, .. } => {
                sql.contains("INSERT INTO knowledge") && sql.contains("file")
            }
            _ => false,
        }
    }));
}

#[tokio::test]
async fn creates_knowledge_for_file_edit() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = RecallStore::new(fake.pool());
    let events = vec![
        make_tool_start(
            "tc-1",
            "file_edit",
            serde_json::json!({"path": "/src/main.rs"}),
        ),
        make_tool_end("tc-1", "file_edit", true, "ok"),
    ];

    process_run_events(&store, "run-1", "user-1", &events).await;

    let calls = fake.calls();
    assert!(calls.iter().any(|call| {
        match call {
            FakeDatabendCall::Query { sql, .. } => {
                sql.contains("INSERT INTO knowledge") && sql.contains("/src/main.rs")
            }
            _ => false,
        }
    }));
}

#[tokio::test]
async fn skips_non_tool_events() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = RecallStore::new(fake.pool());
    let events = vec![Event::Start, Event::ReasonStart];

    process_run_events(&store, "run-1", "user-1", &events).await;

    let calls = fake.calls();
    assert!(calls.is_empty(), "no DB calls expected for non-tool events");
}

#[tokio::test]
async fn skips_file_read_success() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = RecallStore::new(fake.pool());
    let events = vec![
        make_tool_start(
            "tc-1",
            "file_read",
            serde_json::json!({"path": "/tmp/test.rs"}),
        ),
        make_tool_end("tc-1", "file_read", true, "file contents here"),
    ];

    process_run_events(&store, "run-1", "user-1", &events).await;

    let calls = fake.calls();
    assert!(
        calls.is_empty(),
        "file_read success should not produce recall entries"
    );
}

#[tokio::test]
async fn skips_all_failures() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = RecallStore::new(fake.pool());
    let events = vec![
        make_tool_start("tc-1", "shell", serde_json::json!({"command": "foo"})),
        make_tool_end("tc-1", "shell", false, "command not found"),
        make_tool_start("tc-2", "file_write", serde_json::json!({"path": "/tmp/x"})),
        make_tool_end("tc-2", "file_write", false, "permission denied"),
    ];

    process_run_events(&store, "run-1", "user-1", &events).await;

    let calls = fake.calls();
    assert!(
        calls.is_empty(),
        "failures should not produce any recall entries"
    );
}

#[tokio::test]
async fn skips_file_write_without_path() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = RecallStore::new(fake.pool());
    let events = vec![
        make_tool_start("tc-1", "file_write", serde_json::json!({})),
        make_tool_end("tc-1", "file_write", true, "ok"),
    ];

    process_run_events(&store, "run-1", "user-1", &events).await;

    let calls = fake.calls();
    assert!(
        calls.is_empty(),
        "file_write without path should be skipped"
    );
}
