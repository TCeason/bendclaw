use std::path::Path;
use std::path::PathBuf;

use async_trait::async_trait;
use fs2::FileExt;
use tokio::fs;

use crate::error::EvotError;
use crate::error::Result;
use crate::search::collect_search_text;
use crate::search::SessionWithText;
use crate::storage::Storage;
use crate::types::FavoritesDocument;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::VariableRecord;
use crate::types::VariablesDocument;

pub struct FsStorage {
    root_dir: PathBuf,
}

impl FsStorage {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root_dir.join("sessions")
    }

    /// Resolve a session's directory, rejecting IDs that are not well-formed.
    ///
    /// This is the single point where an (possibly untrusted) session ID is
    /// joined to a filesystem path, so the validation that prevents path
    /// traversal lives here and covers every read and write path builder.
    fn session_dir(&self, session_id: &str) -> Result<PathBuf> {
        if !crate::types::is_valid_id(session_id) {
            return Err(EvotError::Store(format!(
                "invalid session id: {session_id:?}"
            )));
        }
        Ok(self.sessions_dir().join(session_id))
    }

    fn session_meta_path(&self, session_id: &str) -> Result<PathBuf> {
        Ok(self.session_dir(session_id)?.join("session.json"))
    }

    fn transcript_path(&self, session_id: &str) -> Result<PathBuf> {
        Ok(self.session_dir(session_id)?.join("transcript.jsonl"))
    }

    fn variables_path(&self) -> PathBuf {
        self.root_dir.join("variables.json")
    }

    fn favorites_path(&self) -> PathBuf {
        self.root_dir.join("favorites.json")
    }

    async fn write_json<T: serde::Serialize>(&self, path: PathBuf, value: &T) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(value)?;
        fs::write(path, json).await?;
        Ok(())
    }

    async fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<Option<T>> {
        match fs::read_to_string(path).await {
            Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(EvotError::Io(e)),
        }
    }

    async fn append_transcript_batch(
        &self,
        path: PathBuf,
        expected_seq: Option<u64>,
        entries: Vec<TranscriptEntry>,
    ) -> Result<bool> {
        if entries.is_empty() {
            return Ok(true);
        }
        let mut line = serde_json::to_vec(&entries)?;
        line.push(b'\n');

        tokio::task::spawn_blocking(move || -> Result<bool> {
            use std::io::Write;

            let Some(parent) = path.parent() else {
                return Err(EvotError::Store(
                    "transcript path has no parent directory".to_string(),
                ));
            };
            std::fs::create_dir_all(parent)?;
            let lock_path = parent.join("transcript.lock");
            let lock_file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(lock_path)?;
            lock_file.lock_exclusive()?;

            let content = match std::fs::read(&path) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(EvotError::Io(error)),
            };
            let valid_len = if content.is_empty() || content.last() == Some(&b'\n') {
                content.len()
            } else {
                content
                    .iter()
                    .rposition(|byte| *byte == b'\n')
                    .map(|index| index.saturating_add(1))
                    .unwrap_or(0)
            };
            if valid_len != content.len() {
                let file = std::fs::OpenOptions::new().write(true).open(&path)?;
                file.set_len(valid_len as u64)?;
            }

            let content = super::migrate::migrate_if_needed(&path, content[..valid_len].to_vec())?;

            if let Some(expected) = expected_seq {
                let persisted_seq = parse_current_transcript(&content)?
                    .iter()
                    .map(|entry| entry.seq)
                    .max()
                    .unwrap_or(0);
                let first_seq = entries.first().map(|entry| entry.seq).unwrap_or(0);
                if persisted_seq != expected || first_seq != expected.saturating_add(1) {
                    FileExt::unlock(&lock_file)?;
                    return Ok(false);
                }
            }

            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            file.write_all(&line)?;
            FileExt::unlock(&lock_file)?;
            Ok(true)
        })
        .await
        .map_err(|error| EvotError::Store(format!("transcript writer task failed: {error}")))?
    }

    async fn read_transcript(&self, path: PathBuf) -> Result<Vec<TranscriptEntry>> {
        tokio::task::spawn_blocking(move || -> Result<Vec<TranscriptEntry>> {
            let Some(parent) = path.parent() else {
                return Err(EvotError::Store(
                    "transcript path has no parent directory".to_string(),
                ));
            };
            std::fs::create_dir_all(parent)?;
            let lock_file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(parent.join("transcript.lock"))?;
            FileExt::lock_exclusive(&lock_file)?;
            let content = match std::fs::read(&path) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(EvotError::Io(error)),
            };
            let content = super::migrate::migrate_if_needed(&path, content)?;
            let entries = parse_current_transcript(&content)?;
            FileExt::unlock(&lock_file)?;
            Ok(entries)
        })
        .await
        .map_err(|error| EvotError::Store(format!("transcript reader task failed: {error}")))?
    }
}

fn parse_current_transcript(content: &[u8]) -> Result<Vec<TranscriptEntry>> {
    let mut entries = Vec::new();
    for line in content.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let serde_json::Value::Array(_) = value else {
            return Err(EvotError::Store(
                "legacy transcript was not migrated".to_string(),
            ));
        };
        let batch: Vec<TranscriptEntry> = serde_json::from_value(value)?;
        entries.extend(batch);
    }
    Ok(entries)
}

fn validate_transcript_batch(entries: &[TranscriptEntry], session_id: &str) -> Result<()> {
    if entries.iter().any(|entry| entry.session_id != session_id) {
        return Err(EvotError::Store(
            "transcript batch contains multiple session ids".to_string(),
        ));
    }
    if entries
        .windows(2)
        .any(|pair| pair[1].seq != pair[0].seq.saturating_add(1))
    {
        return Err(EvotError::Store(
            "transcript batch sequence numbers are not contiguous".to_string(),
        ));
    }
    Ok(())
}

#[async_trait]
impl Storage for FsStorage {
    async fn save_session(&self, session: SessionMeta) -> Result<()> {
        self.write_json(self.session_meta_path(&session.session_id)?, &session)
            .await
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>> {
        self.read_json(&self.session_meta_path(session_id)?).await
    }

    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>> {
        let mut entries = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(EvotError::Io(e)),
        };

        let mut sessions = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            // Skip non-directory entries (e.g. .DS_Store)
            match entry.file_type().await {
                Ok(ft) if ft.is_dir() => {}
                Ok(_) => continue,
                Err(e) => {
                    tracing::warn!(path = ?entry.path(), "skipping session entry: {e}");
                    continue;
                }
            }
            let path = entry.path().join("session.json");
            match self.read_json::<SessionMeta>(&path).await {
                Ok(Some(session)) => sessions.push(session),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(path = ?path, "skipping malformed session.json: {e}");
                }
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if params.limit > 0 {
            sessions.truncate(params.limit);
        }
        Ok(sessions)
    }

    async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let dir = self.session_dir(session_id)?;
        match fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(EvotError::Io(e)),
        }
    }

    async fn append_entries(&self, entries: Vec<TranscriptEntry>) -> Result<()> {
        let Some(first) = entries.first() else {
            return Ok(());
        };
        validate_transcript_batch(&entries, &first.session_id)?;
        let expected_seq = first.seq.saturating_sub(1);
        if !self
            .append_transcript_batch(
                self.transcript_path(&first.session_id)?,
                Some(expected_seq),
                entries,
            )
            .await?
        {
            return Err(EvotError::Store(format!(
                "transcript sequence conflict: expected seq {expected_seq}"
            )));
        }
        Ok(())
    }

    async fn compare_and_append_entries(
        &self,
        expected_seq: u64,
        entries: Vec<TranscriptEntry>,
    ) -> Result<bool> {
        let Some(first) = entries.first() else {
            return Ok(true);
        };
        validate_transcript_batch(&entries, &first.session_id)?;
        self.append_transcript_batch(
            self.transcript_path(&first.session_id)?,
            Some(expected_seq),
            entries,
        )
        .await
    }

    async fn list_entries(&self, params: ListTranscriptEntries) -> Result<Vec<TranscriptEntry>> {
        let mut entries = self
            .read_transcript(self.transcript_path(&params.session_id)?)
            .await?;

        if let Some(run_id) = &params.run_id {
            entries.retain(|entry| entry.run_id.as_ref() == Some(run_id));
        }
        if let Some(after_seq) = params.after_seq {
            entries.retain(|entry| entry.seq > after_seq);
        }
        if let Some(limit) = params.limit {
            entries.truncate(limit);
        }

        Ok(entries)
    }

    async fn load_variables(&self) -> Result<Vec<VariableRecord>> {
        match self
            .read_json::<VariablesDocument>(&self.variables_path())
            .await?
        {
            Some(doc) => Ok(doc.variables),
            None => Ok(Vec::new()),
        }
    }

    async fn save_variables(&self, variables: Vec<VariableRecord>) -> Result<()> {
        let doc = VariablesDocument {
            version: 1,
            variables,
        };
        self.write_json(self.variables_path(), &doc).await
    }

    async fn load_favorites(&self) -> Result<Vec<String>> {
        match self
            .read_json::<FavoritesDocument>(&self.favorites_path())
            .await?
        {
            Some(doc) => Ok(doc.ids),
            None => Ok(Vec::new()),
        }
    }

    async fn save_favorites(&self, ids: Vec<String>) -> Result<()> {
        let doc = FavoritesDocument { version: 1, ids };
        self.write_json(self.favorites_path(), &doc).await
    }

    async fn list_sessions_with_text(&self, limit: usize) -> Result<Vec<SessionWithText>> {
        let sessions = self.list_sessions(ListSessions { limit }).await?;
        let mut result = Vec::with_capacity(sessions.len());

        for session in &sessions {
            let entries: Vec<TranscriptEntry> = match self.transcript_path(&session.session_id) {
                Ok(path) => match self.read_transcript(path).await {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!(
                            session_id = %session.session_id,
                            "skipping transcript: {e}"
                        );
                        vec![]
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        session_id = %session.session_id,
                        "skipping session with invalid id: {e}"
                    );
                    vec![]
                }
            };
            let search_text = collect_search_text(session, &entries);
            result.push(SessionWithText {
                session: session.clone(),
                search_text,
            });
        }

        Ok(result)
    }
}
