//! Cloud → local. Appends only what is missing, through the storage layer's
//! own compare-and-append so a run writing concurrently cannot be interleaved.

use std::sync::Arc;

use chrono::Utc;

use super::client;
use super::types::RemoteSession;
use super::types::SyncPull;
use crate::auth::AuthState;
use crate::error::EvotError;
use crate::error::Result;
use crate::storage::Storage;
use crate::types::new_id;
use crate::types::CloudSync;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;

#[derive(Debug, Clone)]
pub enum PullOutcome {
    /// Local copy now matches the server at `cloud.synced_seq`.
    Pulled {
        meta: Box<SessionMeta>,
        appended: usize,
    },
    /// Nothing new on the server.
    UpToDate,
    /// Local has entries the server has not seen; a push will catch it up.
    LocalAhead { local_seq: u64, remote_seq: u64 },
    /// Both sides appended past the last common point.
    Diverged { local_seq: u64, remote_seq: u64 },
}

/// How a local session relates to the server copy, computed from metadata
/// alone so the `/sessions` list stays cheap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudState {
    /// Not on the cloud.
    Local,
    /// In step with the server.
    Synced,
    /// Local changed since the last acknowledged push.
    PushPending,
    /// Server moved past what this machine has.
    PullPending,
    /// Both of the above.
    Diverged,
    /// Only on the server; nothing local yet.
    RemoteOnly,
}

pub fn cloud_state(local: Option<&SessionMeta>, remote: Option<&RemoteSession>) -> CloudState {
    let Some(local) = local else {
        return if remote.is_some() {
            CloudState::RemoteOnly
        } else {
            CloudState::Local
        };
    };
    let Some(cloud) = local.cloud.as_ref() else {
        return CloudState::Local;
    };
    let push_pending = local.updated_at > cloud.synced_at;
    let pull_pending = remote.is_some_and(|remote| remote.seq > cloud.synced_seq);
    match (push_pending, pull_pending) {
        (true, true) => CloudState::Diverged,
        (true, false) => CloudState::PushPending,
        (false, true) => CloudState::PullPending,
        (false, false) => CloudState::Synced,
    }
}

pub async fn pull_session(
    state: &AuthState,
    storage: &Arc<dyn Storage>,
    session_id: &str,
) -> Result<PullOutcome> {
    let local = storage.get_session(session_id).await?;
    let local_seq = last_seq(storage, session_id).await?;
    let synced_seq = local
        .as_ref()
        .and_then(|meta| meta.cloud.as_ref())
        .map(|cloud| cloud.synced_seq)
        .unwrap_or(0);

    // Local entries the server has not acknowledged: fetch nothing we would
    // have to throw away, just learn where the server is.
    if local.is_some() && local_seq > synced_seq {
        let remote = client::pull(state, session_id, local_seq).await?;
        return Ok(if remote.seq > synced_seq {
            PullOutcome::Diverged {
                local_seq,
                remote_seq: remote.seq,
            }
        } else {
            PullOutcome::LocalAhead {
                local_seq,
                remote_seq: remote.seq,
            }
        });
    }

    let remote = client::pull(state, session_id, local_seq).await?;
    if local.is_some() && remote.seq <= local_seq {
        return Ok(PullOutcome::UpToDate);
    }
    let appended = remote.entries.len();
    let meta = install(storage, session_id, local.is_some(), local_seq, remote).await?;
    Ok(PullOutcome::Pulled {
        meta: Box::new(meta),
        appended,
    })
}

/// Materialise the server copy under a fresh local id, leaving both the
/// diverged local session and the cloud copy untouched. The fork is local-only.
pub async fn fork_remote_session(
    state: &AuthState,
    storage: &Arc<dyn Storage>,
    session_id: &str,
) -> Result<SessionMeta> {
    let remote = client::pull(state, session_id, 0).await?;
    let new_id = new_id();
    let mut meta = remote.meta.clone();
    meta.session_id = new_id.clone();
    meta.cloud = None;
    meta.title = Some(format!(
        "{} (cloud copy)",
        meta.display_title().unwrap_or("session")
    ));
    let entries: Vec<TranscriptEntry> = remote
        .entries
        .into_iter()
        .map(|mut entry| {
            entry.session_id = new_id.clone();
            entry
        })
        .collect();
    storage.save_session(meta.clone()).await?;
    if !entries.is_empty() {
        storage.append_entries(entries).await?;
    }
    Ok(meta)
}

async fn install(
    storage: &Arc<dyn Storage>,
    session_id: &str,
    exists: bool,
    local_seq: u64,
    remote: SyncPull,
) -> Result<SessionMeta> {
    let cloud = CloudSync {
        visibility: remote.visibility,
        synced_seq: remote.seq,
        synced_at: Utc::now().to_rfc3339(),
        origin_host: remote.origin_host,
        public_url: remote.public_url,
        team: remote.team && remote.visibility == crate::types::CloudVisibility::Private,
        team_url: remote.team_url,
        team_name: remote.team_name,
    };
    let mut meta = remote.meta;
    meta.session_id = session_id.to_string();
    if !exists {
        // Save carries `cloud` only on first write; later saves preserve it.
        meta.cloud = Some(cloud.clone());
        storage.save_session(meta.clone()).await?;
    }
    if !remote.entries.is_empty() {
        let appended = storage
            .compare_and_append_entries(local_seq, remote.entries)
            .await?;
        if !appended {
            return Err(EvotError::Store(
                "session changed locally while pulling; retry".into(),
            ));
        }
    }
    if exists {
        storage.save_session(meta).await?;
    }
    // Sync state is written last: an interrupted pull re-pulls, never skips.
    // `synced_at` is stamped after the meta save, so the pulled session reads
    // as in step rather than "changed since last push".
    storage.set_session_cloud(session_id, Some(cloud)).await
}

async fn last_seq(storage: &Arc<dyn Storage>, session_id: &str) -> Result<u64> {
    let entries = storage
        .list_entries(ListTranscriptEntries {
            session_id: session_id.to_string(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    Ok(entries.last().map(|entry| entry.seq).unwrap_or(0))
}
