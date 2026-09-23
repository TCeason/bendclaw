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
    /// A private session that signed-in members of the owner's group can
    /// also read (`/share team`). Additive: builds that predate it ignore the
    /// flag and see a plain private session, and their next push turns the
    /// team page off. An old machine can narrow access, never widen it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub team: bool,
    /// Team page, present while `team` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_url: Option<String>,
    /// Name of the group the team page is shared with, for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
}

/// Who can read a cloud session, as `/share` names it. Stored as
/// `visibility` plus `team` so the published `visibility` keeps its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudAccess {
    /// The owner's machines only.
    Private,
    /// Also signed-in members of the owner's group.
    Team,
    /// Also anyone with the link.
    Public,
}

impl CloudSync {
    pub fn new(visibility: CloudVisibility, origin_host: impl Into<String>) -> Self {
        Self {
            visibility,
            synced_seq: 0,
            synced_at: String::new(),
            origin_host: origin_host.into(),
            public_url: None,
            team: false,
            team_url: None,
            team_name: None,
        }
    }

    pub fn access(&self) -> CloudAccess {
        match (self.visibility, self.team) {
            (CloudVisibility::Public, _) => CloudAccess::Public,
            (CloudVisibility::Private, true) => CloudAccess::Team,
            (CloudVisibility::Private, false) => CloudAccess::Private,
        }
    }

    pub fn set_access(&mut self, access: CloudAccess) {
        self.visibility = match access {
            CloudAccess::Public => CloudVisibility::Public,
            CloudAccess::Private | CloudAccess::Team => CloudVisibility::Private,
        };
        self.team = access == CloudAccess::Team;
    }
}
