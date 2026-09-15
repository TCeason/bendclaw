//! Executor identity and capability registration.

use crate::auth::AuthState;
use crate::conf::ChannelsConfig;
use crate::error::Result;

/// Executor identity is shared by every instance of one cloud user.
///
/// A run becomes claimable by whichever live instance polls for it — and the
/// only process that polls is the one that bound the embedded server's port,
/// so "the :8082 owner fetches the work" holds per machine and across
/// machines. Deriving the id from `hostname` or an instance name orphaned
/// tasks the moment the network renamed the host: nothing running could
/// still claim them.
pub fn executor_id(user_id: &str) -> String {
    use sha2::Digest;

    let digest = sha2::Sha256::digest(user_id.as_bytes());
    format!("exec_{}", hex(&digest[..12]))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn executor_name(executor_id: &str) -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| executor_id.to_string())
}

/// What this device can do for scheduled tasks right now. Recomputed from
/// config on every poll so console edits take effect without a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorCapabilities {
    pub feishu_ready: bool,
}

impl ExecutorCapabilities {
    pub fn from_channels(channels: &ChannelsConfig) -> Self {
        Self {
            feishu_ready: channels.feishu.as_ref().is_some_and(|feishu| {
                !feishu.app_id.trim().is_empty() && !feishu.app_secret.trim().is_empty()
            }),
        }
    }

    fn wire(&self) -> serde_json::Value {
        serde_json::json!({"feishu": {"ready": self.feishu_ready}})
    }

    /// Identity of one registration payload. Registration is repeated only when
    /// this changes, so a steady state costs no extra requests.
    pub fn fingerprint(&self, user_id: &str, executor_id: &str, name: &str) -> String {
        format!(
            "{user_id}|{executor_id}|{name}|feishu={}",
            self.feishu_ready
        )
    }
}

pub async fn register_executor(
    auth: &AuthState,
    id: &str,
    name: &str,
    capabilities: &ExecutorCapabilities,
) -> Result<()> {
    super::client::register_executor(auth, id, name, capabilities.wire()).await
}
