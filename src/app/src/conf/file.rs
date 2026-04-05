use std::path::Path;

use serde::Deserialize;

use crate::conf::paths;
use crate::conf::Config;
use crate::conf::ProviderKind;
use crate::conf::StoreBackend;
use crate::error::BendclawError;
use crate::error::Result;

fn optional_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ConfigPatch {
    llm: LlmSelectionPatch,
    anthropic: ProviderPatch,
    openai: ProviderPatch,
    server: ServerPatch,
    store: StorePatch,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LlmSelectionPatch {
    provider: Option<ProviderKind>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProviderPatch {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerPatch {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StorePatch {
    backend: Option<StoreBackend>,
    fs: FsStorePatch,
    cloud: CloudStorePatch,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FsStorePatch {
    root_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CloudStorePatch {
    endpoint: Option<String>,
    api_key: Option<String>,
    workspace: Option<String>,
}

pub fn load_file_patch(path: &Path) -> Result<ConfigPatch> {
    if !path.exists() {
        return Ok(ConfigPatch::default());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| BendclawError::Conf(format!("failed to read {}: {e}", path.display())))?;

    let parser = toml::Deserializer::new(&content);
    serde_ignored::deserialize(parser, |unknown| {
        tracing::warn!(path = %unknown, "unknown config field");
    })
    .map_err(|e| BendclawError::Conf(format!("failed to parse {}: {e}", path.display())))
}

impl ConfigPatch {
    pub fn apply(self, config: &mut Config) -> Result<()> {
        if let Some(provider) = self.llm.provider {
            config.llm.provider = provider;
        }

        if let Some(api_key) = self.anthropic.api_key {
            config.anthropic.api_key = api_key;
        }
        if let Some(base_url) = self.anthropic.base_url {
            config.anthropic.base_url = optional_string(base_url);
        }
        if let Some(model) = self.anthropic.model {
            config.anthropic.model = model;
        }

        if let Some(api_key) = self.openai.api_key {
            config.openai.api_key = api_key;
        }
        if let Some(base_url) = self.openai.base_url {
            config.openai.base_url = optional_string(base_url);
        }
        if let Some(model) = self.openai.model {
            config.openai.model = model;
        }

        if let Some(host) = self.server.host {
            config.server.host = host;
        }
        if let Some(port) = self.server.port {
            config.server.port = port;
        }

        if let Some(backend) = self.store.backend {
            config.store.backend = backend;
        }
        if let Some(root_dir) = self.store.fs.root_dir {
            config.store.fs.root_dir = paths::expand_home_path(&root_dir)?;
        }
        if let Some(endpoint) = self.store.cloud.endpoint {
            config.store.cloud.endpoint = endpoint;
        }
        if let Some(api_key) = self.store.cloud.api_key {
            config.store.cloud.api_key = api_key;
        }
        if let Some(workspace) = self.store.cloud.workspace {
            config.store.cloud.workspace = optional_string(workspace);
        }

        Ok(())
    }
}
