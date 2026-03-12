use bendclaw::storage::dal::learning::LearningRecord;

#[test]
fn learning_record_serde_roundtrip() {
    let record = LearningRecord {
        id: "l-1".into(),
        kind: "correction".into(),
        subject: "shell".into(),
        title: "shell failure".into(),
        content: "command not found".into(),
        conditions: Some(serde_json::json!({"os": "linux"})),
        strategy: Some(serde_json::json!({"retry": true})),
        priority: 5,
        confidence: 0.8,
        status: "active".into(),
        supersedes_id: String::new(),
        user_id: "user-1".into(),
        source_run_id: "run-1".into(),
        success_count: 3,
        failure_count: 1,
        last_applied_at: Some("2026-03-10T00:00:00Z".into()),
        created_at: "2026-03-10T00:00:00Z".into(),
        updated_at: "2026-03-11T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: LearningRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "l-1");
    assert_eq!(parsed.kind, "correction");
    assert_eq!(parsed.priority, 5);
    assert_eq!(parsed.confidence, 0.8);
    assert_eq!(parsed.success_count, 3);
    assert_eq!(parsed.failure_count, 1);
    assert!(parsed.conditions.is_some());
    assert!(parsed.strategy.is_some());
    assert_eq!(
        parsed.last_applied_at.as_deref(),
        Some("2026-03-10T00:00:00Z")
    );
}

#[test]
fn learning_record_serde_null_optionals() {
    let record = LearningRecord {
        id: "l-2".into(),
        kind: "pattern".into(),
        subject: String::new(),
        title: "test pattern".into(),
        content: "always check errors".into(),
        conditions: None,
        strategy: None,
        priority: 0,
        confidence: 1.0,
        status: "active".into(),
        supersedes_id: String::new(),
        user_id: "user-1".into(),
        source_run_id: String::new(),
        success_count: 0,
        failure_count: 0,
        last_applied_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: LearningRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "l-2");
    assert!(parsed.conditions.is_none());
    assert!(parsed.strategy.is_none());
    assert!(parsed.last_applied_at.is_none());
}
