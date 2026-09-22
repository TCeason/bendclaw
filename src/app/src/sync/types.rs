//! Wire types for `/v1/sessions`, the private cross-machine session store.
//!
//! Distinct from `/v1/shares`: shares are public, lossy, immutable snapshots;
//! this is the owner's own transcript, kept whole so any signed-in machine can
//! resume it. Every payload carries `schema_version`; the server refuses
//! versions it does not know rather than guessing.

use serde::Deserialize;
use serde::Serialize;

use crate::types::CloudVisibility;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;

pub const SYNC_SCHEMA_VERSION: u32 = 1;

/// One incremental push. `entries` are strictly after `expected_seq`; the
/// server appends them only when its copy still ends at `expected_seq`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPush {
    pub schema_version: u32,
    pub evot_version: String,
    /// Full metadata with `cloud` stripped: sync state is per machine and is
    /// rebuilt from the server's answer on the pulling side.
    pub meta: SessionMeta,
    pub expected_seq: u64,
    pub entries: Vec<TranscriptEntry>,
    pub visibility: CloudVisibility,
    pub origin_host: String,
    /// Replace the server copy instead of appending. Only the explicit
    /// "overwrite with local" resolution sets this.
    #[serde(default)]
    pub force: bool,
    /// Full viewer document (same contract as `/v1/shares`), present while
    /// public so the server can render the page without knowing evot's
    /// transcript format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<serde_json::Value>,
}

/// Server acknowledgement for a push or a visibility change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAck {
    pub seq: u64,
    pub visibility: CloudVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    #[serde(default)]
    pub updated_at: String,
}

/// Push result the caller must branch on. A conflict is a normal outcome, not
/// an error: it means another machine appended first.
#[derive(Debug, Clone)]
pub enum PushResponse {
    Acked(SyncAck),
    Conflict { remote_seq: u64 },
}

/// One row of the owner's remote index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSession {
    pub session_id: String,
    pub meta: SessionMeta,
    pub seq: u64,
    pub visibility: CloudVisibility,
    #[serde(default)]
    pub origin_host: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteIndex {
    pub schema_version: u32,
    pub sessions: Vec<RemoteSession>,
}

/// Everything after `after_seq` for one session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPull {
    pub schema_version: u32,
    pub meta: SessionMeta,
    pub seq: u64,
    pub visibility: CloudVisibility,
    #[serde(default)]
    pub origin_host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    pub entries: Vec<TranscriptEntry>,
}
