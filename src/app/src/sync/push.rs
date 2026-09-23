//! Local → cloud. Every push is a compare-and-append at `cloud.synced_seq`, so
//! two machines can never silently interleave; the loser sees `Diverged` and
//! the user picks a side.

use std::sync::Arc;

use chrono::Utc;

use super::client;
use super::types::PushResponse;
use super::types::SyncAck;
use super::types::SyncPush;
use super::types::SYNC_SCHEMA_VERSION;
use crate::auth::AuthState;
use crate::error::EvotError;
use crate::error::Result;
use crate::storage::Storage;
use crate::types::CloudAccess;
use crate::types::CloudSync;
use crate::types::CloudVisibility;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;

#[derive(Debug, Clone)]
pub enum PushOutcome {
    /// Server now ends at `cloud.synced_seq`; `pushed` entries went up.
    Synced { cloud: CloudSync, pushed: usize },
    /// Another machine appended past our `synced_seq`. Nothing was written.
    Diverged { local_seq: u64, remote_seq: u64 },
    /// `SessionMeta.cloud` is `None`: the session is local-only.
    NotShared,
}

pub fn local_host() -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_default()
}

/// Turn cloud sync on (or change who can read it) and push right away, so
/// the command that enabled it reports a real server state rather than a
/// promise.
///
/// `access: None` means "keep what it has": a bare `/share` on a public
/// session must not quietly take the page down. New sessions start private.
pub async fn share_session(
    state: &AuthState,
    storage: &Arc<dyn Storage>,
    session_id: &str,
    access: Option<CloudAccess>,
    evot_version: &str,
) -> Result<PushOutcome> {
    let meta = load_meta(storage, session_id).await?;
    let mut cloud = meta
        .cloud
        .unwrap_or_else(|| CloudSync::new(CloudVisibility::Private, local_host()));
    if let Some(access) = access {
        cloud.set_access(access);
    }
    storage.set_session_cloud(session_id, Some(cloud)).await?;
    let outcome = push_session(state, storage, session_id, evot_version, false).await?;
    // A server that predates team pages ignores the flag and acknowledges a
    // plain private session. Say so instead of reporting success.
    if access == Some(CloudAccess::Team) {
        if let PushOutcome::Synced { cloud, .. } = &outcome {
            if !cloud.team {
                return Err(EvotError::Conf(
                    "the cloud server does not support team sharing yet; the session stays private"
                        .into(),
                ));
            }
        }
    }
    Ok(outcome)
}

/// Incremental push of everything after `cloud.synced_seq`. Metadata always
/// rides along so renames and turn counts reach the other machines even when
/// no new transcript entry exists.
///
/// `force` replaces the server copy wholesale: the "overwrite with local" side
/// of a divergence.
pub async fn push_session(
    state: &AuthState,
    storage: &Arc<dyn Storage>,
    session_id: &str,
    evot_version: &str,
    force: bool,
) -> Result<PushOutcome> {
    let meta = load_meta(storage, session_id).await?;
    let Some(cloud) = meta.cloud.clone() else {
        return Ok(PushOutcome::NotShared);
    };
    let after_seq = if force { 0 } else { cloud.synced_seq };
    let all = storage
        .list_entries(ListTranscriptEntries {
            session_id: session_id.to_string(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    // A page (public or team) needs the whole transcript; the private copy
    // only the tail. Both come from one read so they cannot disagree.
    let access = cloud.access();
    let viewer = (access != CloudAccess::Private && !all.is_empty()).then(|| {
        serde_json::to_value(crate::share::export_session(&meta, &all, evot_version))
            .unwrap_or(serde_json::Value::Null)
    });
    let entries: Vec<_> = all.into_iter().filter(|e| e.seq > after_seq).collect();
    let local_seq = entries.last().map(|e| e.seq).unwrap_or(after_seq);
    let payload = SyncPush {
        schema_version: SYNC_SCHEMA_VERSION,
        evot_version: evot_version.to_string(),
        meta: wire_meta(&meta),
        expected_seq: after_seq,
        entries,
        visibility: cloud.visibility,
        team: access == CloudAccess::Team,
        origin_host: cloud.origin_host.clone(),
        force,
        viewer,
    };
    let pushed = payload.entries.len();
    match client::push(state, &payload).await? {
        PushResponse::Acked(ack) => {
            let cloud = acknowledge(storage, session_id, cloud, &ack).await?;
            Ok(PushOutcome::Synced { cloud, pushed })
        }
        PushResponse::Conflict { remote_seq } => Ok(PushOutcome::Diverged {
            local_seq,
            remote_seq,
        }),
    }
}

/// Remove the server copy and forget sync state locally. The transcript stays.
pub async fn unshare_session(
    state: &AuthState,
    storage: &Arc<dyn Storage>,
    session_id: &str,
) -> Result<()> {
    match client::delete(state, session_id).await {
        Ok(()) => {}
        // Already gone remotely: still clear the local flag.
        Err(EvotError::Conf(message)) if message.contains("HTTP 404") => {}
        Err(error) => return Err(error),
    }
    storage.set_session_cloud(session_id, None).await?;
    Ok(())
}

async fn acknowledge(
    storage: &Arc<dyn Storage>,
    session_id: &str,
    mut cloud: CloudSync,
    ack: &SyncAck,
) -> Result<CloudSync> {
    cloud.synced_seq = ack.seq;
    cloud.synced_at = Utc::now().to_rfc3339();
    cloud.visibility = ack.visibility;
    cloud.public_url = ack.public_url.clone();
    // The server's answer wins: it may have refused or dropped the team page.
    cloud.team = ack.team && ack.visibility == CloudVisibility::Private;
    cloud.team_url = ack.team_url.clone();
    cloud.team_name = ack.team_name.clone();
    let saved = storage
        .set_session_cloud(session_id, Some(cloud.clone()))
        .await?;
    Ok(saved.cloud.unwrap_or(cloud))
}

async fn load_meta(storage: &Arc<dyn Storage>, session_id: &str) -> Result<SessionMeta> {
    storage
        .get_session(session_id)
        .await?
        .ok_or_else(|| EvotError::Session(format!("session not found: {session_id}")))
}

/// Sync state is per machine; the server hands each puller its own view.
pub(super) fn wire_meta(meta: &SessionMeta) -> SessionMeta {
    let mut meta = meta.clone();
    meta.cloud = None;
    meta
}
