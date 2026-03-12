use anyhow::Result;
use bendclaw::storage::databases::AgentDatabases;
use bendclaw::storage::Pool;

#[test]
fn agent_database_name() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bendclaw_")?;
    assert_eq!(dbs.agent_database_name("my-agent")?, "bendclaw_my-agent");
    Ok(())
}

#[test]
fn prefix_accessor() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "test_")?;
    assert_eq!(dbs.prefix(), "test_");
    Ok(())
}

#[test]
fn empty_prefix_rejected() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let result = AgentDatabases::new(pool, "");
    assert!(result.is_err());
    Ok(())
}

#[test]
fn invalid_prefix_rejected() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let result = AgentDatabases::new(pool, "bad prefix!");
    assert!(result.is_err());
    Ok(())
}

#[test]
fn valid_prefix_with_hyphen() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let result = AgentDatabases::new(pool, "my-prefix");
    assert!(result.is_ok());
    Ok(())
}

#[test]
fn agent_pool_creates_scoped_pool() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    let agent_pool = dbs.agent_pool("test-agent")?;
    assert_eq!(agent_pool.base_url(), "https://app.databend.com/v1");
    Ok(())
}

// ── agent_database_name validation ───────────────────────────────────────────

#[test]
fn agent_database_name_preserves_case() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert_eq!(dbs.agent_database_name("MyAgent")?, "bc_MyAgent");
    Ok(())
}

#[test]
fn agent_database_name_allows_hyphen_and_underscore() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert_eq!(dbs.agent_database_name("a--b_c")?, "bc_a--b_c");
    Ok(())
}

#[test]
fn agent_database_name_rejects_special_chars() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert!(dbs.agent_database_name("agent@company!v2").is_err());
    Ok(())
}

#[test]
fn agent_database_name_rejects_empty_agent_id() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert!(dbs.agent_database_name("").is_err());
    Ok(())
}

#[test]
fn agent_database_name_rejects_whitespace_agent_id() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert!(dbs.agent_database_name("   ").is_err());
    Ok(())
}

#[test]
fn agent_database_name_numeric_agent_id() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert_eq!(dbs.agent_database_name("42")?, "bc_42");
    Ok(())
}

#[test]
fn agent_database_name_rejects_surrounding_whitespace() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert!(dbs.agent_database_name("  agent  ").is_err());
    Ok(())
}

// ── validate_prefix edge cases ───────────────────────────────────────────────

#[test]
fn prefix_with_dot_rejected() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    assert!(AgentDatabases::new(pool, "my.prefix").is_err());
    Ok(())
}

#[test]
fn prefix_with_slash_rejected() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    assert!(AgentDatabases::new(pool, "my/prefix").is_err());
    Ok(())
}

#[test]
fn prefix_with_unicode_rejected() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    assert!(AgentDatabases::new(pool, "前缀_").is_err());
    Ok(())
}

#[test]
fn prefix_all_numeric_accepted() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    assert!(AgentDatabases::new(pool, "123").is_ok());
    Ok(())
}

#[test]
fn prefix_single_char_accepted() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    assert!(AgentDatabases::new(pool, "x").is_ok());
    Ok(())
}

#[test]
fn prefix_underscores_only_accepted() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    assert!(AgentDatabases::new(pool, "___").is_ok());
    Ok(())
}

// ── root_pool accessor ───────────────────────────────────────────────────────

#[test]
fn root_pool_returns_original_pool() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "token", "default")?;
    let dbs = AgentDatabases::new(pool, "bc_")?;
    assert_eq!(dbs.root_pool().base_url(), "https://app.databend.com/v1");
    Ok(())
}
