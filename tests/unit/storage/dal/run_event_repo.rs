use anyhow::Result;
use bendclaw::storage::RunEventRecord;
use bendclaw::storage::RunEventRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

#[tokio::test]
async fn run_event_repo_insert_batch_and_list_by_run() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("INSERT INTO run_events ") {
            assert!(sql.contains("'evt-1'"));
            assert!(sql.contains("'RunStarted'"));
            assert!(sql.contains("'evt-2'"));
            assert!(sql.contains("'RunCompleted'"));
            return Ok(paged_rows(&[], None, None));
        }
        assert_eq!(
            sql,
            "SELECT id, run_id, session_id, agent_id, user_id, seq, event, payload, TO_VARCHAR(created_at) FROM run_events WHERE run_id = 'run-1' ORDER BY seq ASC, created_at ASC LIMIT 10"
        );
        Ok(paged_rows(
            &[&[
                "evt-1",
                "run-1",
                "session-1",
                "agent-1",
                "user-1",
                "1",
                "RunStarted",
                "{\"event\":\"RunStarted\"}",
                "2026-03-11T00:00:00Z",
            ]],
            None,
            None,
        ))
    });
    let repo = RunEventRepo::new(fake.pool());

    repo.insert_batch(&[
        RunEventRecord {
            id: "evt-1".into(),
            run_id: "run-1".into(),
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            user_id: "user-1".into(),
            seq: 1,
            event: "RunStarted".into(),
            payload: "{\"event\":\"RunStarted\"}".into(),
            created_at: String::new(),
        },
        RunEventRecord {
            id: "evt-2".into(),
            run_id: "run-1".into(),
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            user_id: "user-1".into(),
            seq: 2,
            event: "RunCompleted".into(),
            payload: "{\"event\":\"RunCompleted\"}".into(),
            created_at: String::new(),
        },
    ])
    .await?;

    let events = repo.list_by_run("run-1", 10).await?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "evt-1");
    assert_eq!(events[0].event, "RunStarted");
    assert_eq!(
        fake.calls(),
        vec![
            FakeDatabendCall::Query {
                sql: "INSERT INTO run_events (id, run_id, session_id, agent_id, user_id, seq, event, payload, created_at) VALUES ('evt-1', 'run-1', 'session-1', 'agent-1', 'user-1', 1, 'RunStarted', '{\"event\":\"RunStarted\"}', NOW()), ('evt-2', 'run-1', 'session-1', 'agent-1', 'user-1', 2, 'RunCompleted', '{\"event\":\"RunCompleted\"}', NOW())".to_string(),
                database: None,
            },
            FakeDatabendCall::Query {
                sql: "SELECT id, run_id, session_id, agent_id, user_id, seq, event, payload, TO_VARCHAR(created_at) FROM run_events WHERE run_id = 'run-1' ORDER BY seq ASC, created_at ASC LIMIT 10".to_string(),
                database: None,
            },
        ]
    );
    Ok(())
}
