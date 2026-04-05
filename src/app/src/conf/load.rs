use std::collections::HashMap;

use crate::conf::default_config;
use crate::conf::env;
use crate::conf::file::load_file_patch;
use crate::conf::paths;
use crate::conf::Config;
use crate::conf::ConfigOverrides;
use crate::conf::LlmConfig;
use crate::error::Result;

fn apply_overrides(config: &mut Config, overrides: &ConfigOverrides) {
    if let Some(model) = overrides.model.clone() {
        config
            .provider_config_mut(&config.llm.provider.clone())
            .model = model;
    }
    if let Some(port) = overrides.port {
        config.server.port = port;
    }
}

pub fn resolve_llm_config(
    vars: &HashMap<String, String>,
    cli_model: Option<&str>,
) -> Result<LlmConfig> {
    let mut config = default_config()?;
    env::apply_env(&mut config, vars)?;
    if let Some(model) = cli_model {
        config
            .provider_config_mut(&config.llm.provider.clone())
            .model = model.to_string();
    }
    config.validate()?;
    Ok(config.active_llm())
}

pub fn load_config(overrides: ConfigOverrides) -> Result<Config> {
    let mut config = default_config()?;

    let config_patch = load_file_patch(&paths::config_file_path()?)?;
    config_patch.apply(&mut config)?;

    let env_file_vars = env::load_env_file(&paths::env_file_path()?)?;
    env::apply_env(&mut config, &env_file_vars)?;

    let process_vars = env::load_process_env();
    env::apply_env(&mut config, &process_vars)?;

    apply_overrides(&mut config, &overrides);
    config.validate()?;

    Ok(config)
}
