//! Tests for runtime thinking-level cycling (Agent::cycle_thinking_level)
//! and session-level persistence (Session thinking_level round-trip).

use std::sync::Arc;

use evot::agent::session::Session;
use evot::agent::Agent;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
use evot_engine::provider::CompatCaps;
use evot_engine::ThinkingLevel;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn anthropic_config(dir: &TempDir) -> Config {
    let mut config = Config::new(dir.path().to_path_buf());
    config
        .providers
        .insert("anthropic".into(), ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "test-key".into(),
            base_url: "https://api.anthropic.com".into(),
            models: vec!["claude-opus-4-6".into()],
            compat_caps: Default::default(),
            thinking_level: None,
            context_window: None,
            max_tokens: None,
        });
    config.llm.provider = "anthropic".into();
    config
}

#[test]
fn cycle_thinking_level_anthropic_walks_full_ramp_and_wraps() -> TestResult {
    let dir = TempDir::new()?;
    let agent = Agent::new(&anthropic_config(&dir), "/work")?;

    // Adaptive (default) is not a stop in the cycle, so the first press lands
    // on the first supported level.
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Off));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Low));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Medium));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::High));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Xhigh));
    // Wraps back to the start.
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Off));
    Ok(())
}

#[test]
fn cycle_thinking_level_starts_from_current_level() -> TestResult {
    let dir = TempDir::new()?;
    let agent = Agent::new(&anthropic_config(&dir), "/work")?;

    agent.set_thinking_level(ThinkingLevel::Medium);
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::High));
    Ok(())
}

#[test]
fn cycle_thinking_level_openai_without_effort_capability_is_inert() -> TestResult {
    let dir = TempDir::new()?;
    let mut config = Config::new(dir.path().to_path_buf());
    // A "deepseek" OpenAI-compat provider does not advertise reasoning effort.
    config.providers.insert("deepseek".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into()],
        compat_caps: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
    });
    config.llm.provider = "deepseek".into();
    let agent = Agent::new(&config, "/work")?;

    assert!(agent.supported_thinking_levels().is_empty());
    assert_eq!(agent.cycle_thinking_level(), None);
    Ok(())
}

#[test]
fn cycle_thinking_level_openai_with_effort_capability() -> TestResult {
    let dir = TempDir::new()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("openai".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "https://api.openai.com/v1".into(),
        models: vec!["gpt-5.5".into()],
        compat_caps: CompatCaps::REASONING_EFFORT,
        thinking_level: None,
        context_window: None,
        max_tokens: None,
    });
    config.llm.provider = "openai".into();
    let agent = Agent::new(&config, "/work")?;

    // gpt-5.5 maps xhigh explicitly, so the full ramp is selectable.
    assert_eq!(agent.supported_thinking_levels(), vec![
        ThinkingLevel::Off,
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::Xhigh,
    ]);
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Off));
    Ok(())
}

#[test]
fn cycle_thinking_level_gpt_5_5_pro_cycles_medium_high_xhigh() -> TestResult {
    let dir = TempDir::new()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("openai".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "https://api.openai.com/v1".into(),
        models: vec!["gpt-5.5-pro".into()],
        compat_caps: CompatCaps::REASONING_EFFORT,
        thinking_level: None,
        context_window: None,
        max_tokens: None,
    });
    config.llm.provider = "openai".into();
    let agent = Agent::new(&config, "/work")?;

    // gpt-5.5-pro rejects off/minimal/low; medium is the floor.
    assert_eq!(agent.supported_thinking_levels(), vec![
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::Xhigh,
    ]);
    // Cycling wraps within the restricted ramp.
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Medium));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::High));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Xhigh));
    assert_eq!(agent.cycle_thinking_level(), Some(ThinkingLevel::Medium));
    Ok(())
}

#[test]
fn restore_thinking_level_applies_supported_name() -> TestResult {
    let dir = TempDir::new()?;
    let agent = Agent::new(&anthropic_config(&dir), "/work")?;

    agent.restore_thinking_level("high");
    assert_eq!(agent.llm().thinking_level, ThinkingLevel::High);
    Ok(())
}

#[test]
fn restore_thinking_level_ignores_unknown_or_unsupported() -> TestResult {
    let dir = TempDir::new()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("openai".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "https://api.openai.com/v1".into(),
        models: vec!["gpt-5.5-pro".into()],
        compat_caps: CompatCaps::REASONING_EFFORT,
        thinking_level: None,
        context_window: None,
        max_tokens: None,
    });
    config.llm.provider = "openai".into();
    let agent = Agent::new(&config, "/work")?;
    agent.set_thinking_level(ThinkingLevel::High);

    // Unknown name: left untouched.
    agent.restore_thinking_level("bogus");
    assert_eq!(agent.llm().thinking_level, ThinkingLevel::High);

    // Known but unsupported on gpt-5.5-pro (floor is medium): left untouched.
    agent.restore_thinking_level("low");
    assert_eq!(agent.llm().thinking_level, ThinkingLevel::High);

    // Known and supported: applied.
    agent.restore_thinking_level("xhigh");
    assert_eq!(agent.llm().thinking_level, ThinkingLevel::Xhigh);
    Ok(())
}

#[tokio::test]
async fn session_thinking_level_round_trips_through_storage() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    // A fresh session has no recorded level.
    let session = Session::new_with_source(
        "sess-think".into(),
        "/work".into(),
        "claude-opus-4-6".into(),
        "repl",
        storage.clone(),
    )
    .await?;
    assert_eq!(session.meta().await.thinking_level, None);

    // Stamp + persist, mirroring what a run does via resolve_session + save.
    session.set_thinking_level(Some("high".into())).await;
    session.save().await?;

    // Re-open from storage: the level survives.
    let reopened = Session::open("sess-think", storage)
        .await?
        .ok_or("session missing after save")?;
    assert_eq!(
        reopened.meta().await.thinking_level,
        Some("high".to_string())
    );
    Ok(())
}

/// Sessions persisted before the `thinking_level` field existed deserialize
/// with `None` rather than failing (serde default).
#[test]
fn session_meta_without_thinking_level_deserializes() -> TestResult {
    use evot::types::SessionMeta;
    let legacy = r#"{
        "session_id": "old",
        "cwd": "/work",
        "model": "claude-opus-4-6",
        "title": null,
        "turns": 0,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    }"#;
    let meta: SessionMeta = serde_json::from_str(legacy)?;
    assert_eq!(meta.thinking_level, None);
    let _ = Arc::new(meta);
    Ok(())
}
