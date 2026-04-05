use chrono::Utc;

use crate::error::Result;
use crate::session::SessionState;
use crate::storage::model::TranscriptEntry;
use crate::storage::Storage;

pub fn update_transcript(state: &mut SessionState, messages: Vec<bend_agent::Message>) {
    state.messages = messages;
    state.meta.turns += 1;
    state.meta.updated_at = Utc::now().to_rfc3339();
}

pub async fn save_transcript(state: &SessionState, storage: &dyn Storage) -> Result<()> {
    let entries = state
        .messages
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, message)| {
            TranscriptEntry::new(
                state.meta.session_id.clone(),
                None,
                idx as u64 + 1,
                state.meta.turns,
                message,
            )
        })
        .collect();

    storage.put_session(state.meta.clone()).await?;
    storage.put_transcript_entries(entries).await?;
    Ok(())
}
