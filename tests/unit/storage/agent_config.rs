use anyhow::Result;
use bendclaw::storage::AgentConfigRecord;

#[test]
fn agent_config_record_token_limits() {
    let rec = AgentConfigRecord {
        agent_id: "a1".into(),
        system_prompt: "".into(),
        identity: "".into(),
        soul: "".into(),
        token_limit_total: Some(500000),
        token_limit_daily: Some(50000),
        llm_config: None,
        updated_at: "".into(),
    };
    assert_eq!(rec.token_limit_total, Some(500000));
    assert_eq!(rec.token_limit_daily, Some(50000));
}

#[test]
fn agent_config_record_token_limits_none() {
    let rec = AgentConfigRecord {
        agent_id: "a1".into(),
        system_prompt: "".into(),
        identity: "".into(),
        soul: "".into(),
        token_limit_total: None,
        token_limit_daily: None,
        llm_config: None,
        updated_at: "".into(),
    };
    assert!(rec.token_limit_total.is_none());
    assert!(rec.token_limit_daily.is_none());
}

#[test]
fn agent_config_record_serde_roundtrip() -> Result<()> {
    let rec = AgentConfigRecord {
        agent_id: "a1".into(),
        system_prompt: "you are helpful".into(),
        identity: "You are a coding assistant".into(),
        soul: "Be concise and helpful".into(),
        token_limit_total: Some(1_000_000),
        token_limit_daily: None,
        llm_config: None,
        updated_at: "2026-01-02".into(),
    };
    let json = serde_json::to_string(&rec)?;
    let back: AgentConfigRecord = serde_json::from_str(&json)?;
    assert_eq!(back.agent_id, "a1");
    assert_eq!(back.system_prompt, "you are helpful");
    assert_eq!(back.identity, "You are a coding assistant");
    assert_eq!(back.soul, "Be concise and helpful");
    assert_eq!(back.token_limit_total, Some(1_000_000));
    assert!(back.token_limit_daily.is_none());
    Ok(())
}
