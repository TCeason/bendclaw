use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use crate::conf::paths;
use crate::conf::Config;
use crate::conf::ProviderKind;
use crate::conf::StoreBackend;
use crate::error::BendclawError;
use crate::error::Result;

const RELEVANT_KEYS: &[&str] = &[
    "BENDCLAW_LLM_PROVIDER",
    "BENDCLAW_ANTHROPIC_API_KEY",
    "BENDCLAW_ANTHROPIC_BASE_URL",
    "BENDCLAW_ANTHROPIC_MODEL",
    "BENDCLAW_OPENAI_API_KEY",
    "BENDCLAW_OPENAI_BASE_URL",
    "BENDCLAW_OPENAI_MODEL",
    "BENDCLAW_SERVER_HOST",
    "BENDCLAW_SERVER_PORT",
    "BENDCLAW_STORE_BACKEND",
    "BENDCLAW_STORE_FS_ROOT_DIR",
    "BENDCLAW_STORE_CLOUD_ENDPOINT",
    "BENDCLAW_STORE_CLOUD_API_KEY",
    "BENDCLAW_STORE_CLOUD_WORKSPACE",
];

pub fn load_env_file(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read(path)
        .map_err(|e| BendclawError::Conf(format!("failed to read {}: {e}", path.display())))?;
    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.map_err(|e| {
            BendclawError::Conf(format!("failed to read line in {}: {e}", path.display()))
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let trimmed = match trimmed.strip_prefix("export ") {
            Some(value) => value,
            None => trimmed,
        };
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !value.is_empty() {
                vars.insert(key, value);
            }
        }
    }

    Ok(vars)
}

pub fn load_process_env() -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for &key in RELEVANT_KEYS {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                vars.insert(key.to_string(), value);
            }
        }
    }
    vars
}

fn provider_keys(provider: &ProviderKind) -> (&'static str, &'static str, &'static str) {
    match provider {
        ProviderKind::Anthropic => (
            "BENDCLAW_ANTHROPIC_API_KEY",
            "BENDCLAW_ANTHROPIC_BASE_URL",
            "BENDCLAW_ANTHROPIC_MODEL",
        ),
        ProviderKind::OpenAi => (
            "BENDCLAW_OPENAI_API_KEY",
            "BENDCLAW_OPENAI_BASE_URL",
            "BENDCLAW_OPENAI_MODEL",
        ),
    }
}

fn apply_provider_env(config: &mut Config, provider: ProviderKind, vars: &HashMap<String, String>) {
    let provider_config = config.provider_config_mut(&provider);
    let (api_key_key, base_url_key, model_key) = provider_keys(&provider);

    if let Some(api_key) = vars.get(api_key_key) {
        provider_config.api_key = api_key.clone();
    }
    if let Some(base_url) = vars.get(base_url_key) {
        provider_config.base_url = Some(base_url.clone());
    }
    if let Some(model) = vars.get(model_key) {
        provider_config.model = model.clone();
    }
}

pub fn apply_env(config: &mut Config, vars: &HashMap<String, String>) -> Result<()> {
    if let Some(provider) = vars.get("BENDCLAW_LLM_PROVIDER") {
        config.llm.provider = ProviderKind::from_str_loose(provider)?;
    }

    apply_provider_env(config, ProviderKind::Anthropic, vars);
    apply_provider_env(config, ProviderKind::OpenAi, vars);

    if let Some(host) = vars.get("BENDCLAW_SERVER_HOST") {
        config.server.host = host.clone();
    }
    if let Some(port) = vars.get("BENDCLAW_SERVER_PORT") {
        config.server.port = port.parse::<u16>().map_err(|e| {
            BendclawError::Conf(format!("invalid BENDCLAW_SERVER_PORT value {port}: {e}"))
        })?;
    }

    if let Some(backend) = vars.get("BENDCLAW_STORE_BACKEND") {
        config.store.backend = match backend.as_str() {
            "fs" => StoreBackend::Fs,
            "cloud" => StoreBackend::Cloud,
            other => {
                return Err(BendclawError::Conf(format!(
                    "unknown BENDCLAW_STORE_BACKEND: {other}"
                )))
            }
        };
    }
    if let Some(root_dir) = vars.get("BENDCLAW_STORE_FS_ROOT_DIR") {
        config.store.fs.root_dir = paths::expand_home_path(root_dir)?;
    }
    if let Some(endpoint) = vars.get("BENDCLAW_STORE_CLOUD_ENDPOINT") {
        config.store.cloud.endpoint = endpoint.clone();
    }
    if let Some(api_key) = vars.get("BENDCLAW_STORE_CLOUD_API_KEY") {
        config.store.cloud.api_key = api_key.clone();
    }
    if let Some(workspace) = vars.get("BENDCLAW_STORE_CLOUD_WORKSPACE") {
        config.store.cloud.workspace = Some(workspace.clone());
    }

    Ok(())
}
