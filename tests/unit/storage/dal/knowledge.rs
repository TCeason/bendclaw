use bendclaw::storage::dal::knowledge::KnowledgeRecord;

#[test]
fn knowledge_record_serde_roundtrip() {
    let record = KnowledgeRecord {
        id: "k-1".into(),
        kind: "file".into(),
        subject: "file_read".into(),
        locator: "/tmp/test.rs".into(),
        title: "file read success".into(),
        summary: "Read file contents".into(),
        metadata: Some(serde_json::json!({"lines": 42})),
        status: "active".into(),
        confidence: 0.95,
        user_id: "user-1".into(),
        first_run_id: "run-1".into(),
        last_run_id: "run-2".into(),
        first_seen_at: "2026-03-10T00:00:00Z".into(),
        last_seen_at: "2026-03-11T00:00:00Z".into(),
        created_at: "2026-03-10T00:00:00Z".into(),
        updated_at: "2026-03-11T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: KnowledgeRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "k-1");
    assert_eq!(parsed.kind, "file");
    assert_eq!(parsed.locator, "/tmp/test.rs");
    assert_eq!(parsed.confidence, 0.95);
    assert_eq!(parsed.metadata.unwrap()["lines"], 42);
}

#[test]
fn knowledge_record_serde_null_metadata() {
    let record = KnowledgeRecord {
        id: "k-2".into(),
        kind: "discovery".into(),
        subject: "web_search".into(),
        locator: String::new(),
        title: "search result".into(),
        summary: "Found something".into(),
        metadata: None,
        status: "active".into(),
        confidence: 1.0,
        user_id: "user-1".into(),
        first_run_id: "run-1".into(),
        last_run_id: "run-1".into(),
        first_seen_at: String::new(),
        last_seen_at: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: KnowledgeRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "k-2");
    assert!(parsed.metadata.is_none());
}
