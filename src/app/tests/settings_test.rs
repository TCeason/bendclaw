use std::path::PathBuf;

use evot::conf::apply_settings;
use evot::conf::config_to_env_groups;
use evot::conf::env_writer::write_grouped;
use evot::conf::env_writer::EnvGroup;
use evot::conf::Config;
use evot::conf::FeishuSettings;
use evot::conf::ProviderSettings;
use evot::conf::SettingsUpdate;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// Flatten all groups into a single key->value map for assertions.
fn flat(groups: &[EnvGroup]) -> std::collections::HashMap<String, String> {
    groups
        .iter()
        .flat_map(|g| g.pairs.iter().cloned())
        .collect()
}

/// Apply `sample_update()` into a fresh config and return the groups derived
/// from it — mirrors what the server does on save.
fn sample_groups() -> Vec<EnvGroup> {
    let mut config = Config::new(std::env::temp_dir());
    if let Err(e) = apply_settings(&mut config, &sample_update()) {
        panic!("apply sample update: {e}");
    }
    config_to_env_groups(&config)
}

fn tmp_env_path(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("evot_settings_test_{tag}_{nanos}.env"));
    p
}

fn sample_update() -> SettingsUpdate {
    SettingsUpdate {
        active_provider: "anthropic".into(),
        thinking_level: Some("high".into()),
        providers: vec![ProviderSettings {
            name: "anthropic".into(),
            protocol: "anthropic".into(),
            api_key: Some("sk-secret-123".into()),
            base_url: "https://api.anthropic.com".into(),
            models: vec!["claude-sonnet-4-6".into(), "claude-opus-4-6".into()],
            thinking_level: Some("xhigh".into()),
        }],
        feishu: Some(FeishuSettings {
            app_id: "cli_app".into(),
            app_secret: Some("feishu-secret".into()),
            mention_only: false,
        }),
    }
}

#[test]
fn settings_serialize_to_expected_env_keys() {
    let map = flat(&sample_groups());
    assert_eq!(
        map.get("EVOT_LLM_PROVIDER").map(String::as_str),
        Some("anthropic")
    );
    assert_eq!(
        map.get("EVOT_LLM_THINKING_LEVEL").map(String::as_str),
        Some("high")
    );
    assert_eq!(
        map.get("EVOT_LLM_ANTHROPIC_MODEL").map(String::as_str),
        Some("claude-sonnet-4-6,claude-opus-4-6")
    );
    assert_eq!(
        map.get("EVOT_LLM_ANTHROPIC_API_KEY").map(String::as_str),
        Some("sk-secret-123")
    );
    assert_eq!(
        map.get("EVOT_LLM_ANTHROPIC_THINKING_LEVEL")
            .map(String::as_str),
        Some("xhigh")
    );
    assert_eq!(
        map.get("EVOT_CHANNEL_FEISHU_APP_ID").map(String::as_str),
        Some("cli_app")
    );
    assert_eq!(
        map.get("EVOT_CHANNEL_FEISHU_MENTION_ONLY")
            .map(String::as_str),
        Some("false")
    );
}

#[test]
fn groups_are_titled_and_separated() {
    let groups = sample_groups();
    // One global selection group + one provider group + one Feishu group.
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].title, "Active selection");
    assert_eq!(groups[1].title, "Provider: anthropic");
    assert_eq!(groups[2].title, "Channel: Feishu bot");
}

#[test]
fn empty_secret_is_not_serialized() {
    // A provider/feishu with no stored secret omits the secret key entirely.
    let mut config = Config::new(std::env::temp_dir());
    let mut update = sample_update();
    update.providers[0].api_key = None;
    update.feishu.as_mut().unwrap().app_secret = Some(String::new());
    if let Err(e) = apply_settings(&mut config, &update) {
        panic!("apply: {e}");
    }
    let map = flat(&config_to_env_groups(&config));
    assert!(!map.contains_key("EVOT_LLM_ANTHROPIC_API_KEY"));
    assert!(!map.contains_key("EVOT_CHANNEL_FEISHU_APP_SECRET"));
}

#[test]
fn write_grouped_renders_managed_block_with_headers() -> TestResult {
    let path = tmp_env_path("grouped");
    write_grouped(&path, &sample_groups())?;
    let content = std::fs::read_to_string(&path)?;
    assert!(content.contains("# >>> evot managed"));
    assert!(content.contains("# <<< evot managed"));
    assert!(content.contains("# Active selection"));
    assert!(content.contains("# Provider: anthropic"));
    assert!(content.contains("# Channel: Feishu bot"));
    assert!(content.contains("EVOT_LLM_ANTHROPIC_API_KEY=sk-secret-123"));
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn write_grouped_preserves_user_lines_and_dedupes() -> TestResult {
    let path = tmp_env_path("dedupe");
    // Pre-existing file: a user comment + custom key, plus a stale managed key
    // sitting outside any block that must be deduped away.
    std::fs::write(
        &path,
        "# my notes\nMY_CUSTOM_VAR=keepme\nEVOT_LLM_PROVIDER=stale-openai\n",
    )?;
    write_grouped(&path, &sample_groups())?;
    let content = std::fs::read_to_string(&path)?;
    // User content survives.
    assert!(content.contains("# my notes"));
    assert!(content.contains("MY_CUSTOM_VAR=keepme"));
    // The managed key appears exactly once, with the new value.
    assert_eq!(content.matches("EVOT_LLM_PROVIDER=").count(), 1);
    assert!(content.contains("EVOT_LLM_PROVIDER=anthropic"));
    assert!(!content.contains("stale-openai"));
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn write_grouped_is_idempotent_across_saves() -> TestResult {
    let path = tmp_env_path("idem");
    let groups = sample_groups();
    write_grouped(&path, &groups)?;
    let first = std::fs::read_to_string(&path)?;
    // Saving the same settings again must not grow or duplicate the block.
    write_grouped(&path, &groups)?;
    let second = std::fs::read_to_string(&path)?;
    assert_eq!(first, second);
    assert_eq!(second.matches("# >>> evot managed").count(), 1);
    assert_eq!(second.matches("EVOT_LLM_ANTHROPIC_API_KEY=").count(), 1);
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn resave_keeps_all_managed_keys_inside_block() -> TestResult {
    // Regression: editing a config on the web and saving must not leave any
    // EVOT_* key (especially secrets) stranded outside the managed block.
    let path = tmp_env_path("resave");
    // Simulate a hand-written file with scattered managed keys and a secret.
    std::fs::write(
        &path,
        "EVOT_LLM_ANTHROPIC_API_KEY=old-secret\n\
         EVOT_LLM_DEEPMI_BASE_URL=https://example.com\n\
         EVOT_CHANNEL_FEISHU_APP_SECRET=old-feishu\n\
         EVOT_TELEMETRY_ENDPOINT=http://localhost:3100\n",
    )?;
    write_grouped(&path, &sample_groups())?;
    let content = std::fs::read_to_string(&path)?;

    // Split into preamble (before block) and the managed block.
    let begin = content
        .find("# >>> evot managed")
        .ok_or("managed block missing")?;
    let preamble = &content[..begin];

    // No EVOT_LLM_* or EVOT_CHANNEL_FEISHU_* assignment may sit in the preamble.
    for line in preamble.lines() {
        let t = line.trim();
        assert!(
            !t.starts_with("EVOT_LLM_") && !t.starts_with("EVOT_CHANNEL_FEISHU_"),
            "managed key stranded outside block: {t}"
        );
    }
    // Stale provider + old secrets are gone; the new secret lives in the block.
    assert!(!content.contains("old-secret"));
    assert!(!content.contains("old-feishu"));
    assert!(!content.contains("EVOT_LLM_DEEPMI_BASE_URL"));
    assert_eq!(content.matches("EVOT_LLM_ANTHROPIC_API_KEY=").count(), 1);
    // Foreign keys are preserved.
    assert!(content.contains("EVOT_TELEMETRY_ENDPOINT=http://localhost:3100"));
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn round_trip_persists_and_reloads() -> TestResult {
    let path = tmp_env_path("roundtrip");
    write_grouped(&path, &sample_groups())?;

    let mut config = Config::new(std::env::temp_dir());
    let loaded = Config::load_with_env_file(path.to_str())?;
    // load_with_env_file reads the real default TOML too, but our temp env file
    // drives the provider values we care about here.
    config.providers = loaded.providers;
    config.llm = loaded.llm;

    let anthropic = config
        .providers
        .get("anthropic")
        .ok_or("anthropic provider missing after reload")?;
    assert_eq!(anthropic.api_key, "sk-secret-123");
    assert_eq!(anthropic.model(), "claude-sonnet-4-6");
    assert_eq!(config.llm.provider, "anthropic");
    std::fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn apply_settings_preserves_blank_secret() -> TestResult {
    // Seed a config with an existing key, then apply an update that leaves the
    // api_key blank. The existing secret must survive.
    let mut config = Config::new(std::env::temp_dir());
    let mut seed = sample_update();
    apply_settings(&mut config, &seed)?;
    assert_eq!(
        config
            .providers
            .get("anthropic")
            .map(|p| p.api_key.as_str()),
        Some("sk-secret-123")
    );

    seed.providers[0].api_key = None;
    if let Some(f) = seed.feishu.as_mut() {
        f.app_secret = None;
    }
    apply_settings(&mut config, &seed)?;
    assert_eq!(
        config
            .providers
            .get("anthropic")
            .map(|p| p.api_key.as_str()),
        Some("sk-secret-123")
    );
    assert_eq!(
        config
            .channels
            .feishu
            .as_ref()
            .map(|f| f.app_secret.as_str()),
        Some("feishu-secret")
    );
    Ok(())
}

#[test]
fn apply_settings_rejects_unknown_active_provider() {
    let mut config = Config::new(std::env::temp_dir());
    let mut update = sample_update();
    update.active_provider = "ghost".into();
    assert!(apply_settings(&mut config, &update).is_err());
}

#[test]
fn reloading_env_file_reflects_external_edits() -> TestResult {
    // Regression: the settings page must read the env file fresh, so an edit
    // made outside the dashboard is visible instead of a stale in-memory value.
    // This mirrors what the server's GET /api/settings reload does.
    let path = tmp_env_path("external_edit");
    write_grouped(&path, &sample_groups())?;

    let path_arg = path.to_str();
    let before = Config::load_with_env_file(path_arg)?;
    assert_eq!(
        before
            .providers
            .get("anthropic")
            .map(|p| p.api_key.as_str()),
        Some("sk-secret-123")
    );

    // Someone hand-edits the file, rotating the key.
    let edited = std::fs::read_to_string(&path)?.replace("sk-secret-123", "sk-rotated-999");
    std::fs::write(&path, edited)?;

    // A fresh load (what the page reload does) must see the new value.
    let after = Config::load_with_env_file(path_arg)?;
    assert_eq!(
        after.providers.get("anthropic").map(|p| p.api_key.as_str()),
        Some("sk-rotated-999")
    );
    std::fs::remove_file(&path)?;
    Ok(())
}
