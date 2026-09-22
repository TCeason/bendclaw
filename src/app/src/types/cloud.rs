//! Cloud sync state carried on a session.
//!
//! `SessionMeta.cloud` is owned by the sync path only: run-end saves preserve
//! whatever is on disk, mirroring how `custom_title` is owned by rename. The
//! block travels with the session, so a machine that pulls a shared session
//! keeps syncing it without any extra setup.

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudVisibility {
    /// Only the owner's signed-in machines can list, pull and resume it.
    Private,
    /// Also rendered as a read-only web page that follows the transcript.
    Public,
}

impl CloudVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            CloudVisibility::Private => "private",
            CloudVisibility::Public => "public",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSync {
    pub visibility: CloudVisibility,
    /// Last transcript `seq` the server has acknowledged. `0` before the first
    /// push; the next push sends only entries after it and asks the server to
    /// compare-and-append at exactly this position.
    #[serde(default)]
    pub synced_seq: u64,
    /// When `synced_seq` was acknowledged. `updated_at > synced_at` means the
    /// session changed locally since, i.e. a push is pending.
    #[serde(default)]
    pub synced_at: String,
    /// Where the session was first shared from, shown next to remote rows.
    #[serde(default)]
    pub origin_host: String,
    /// Read-only page, present while `visibility == Public`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
}

impl CloudSync {
    pub fn new(visibility: CloudVisibility, origin_host: impl Into<String>) -> Self {
        Self {
            visibility,
            synced_seq: 0,
            synced_at: String::new(),
            origin_host: origin_host.into(),
            public_url: None,
        }
    }
}
