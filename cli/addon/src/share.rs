use napi::Error;
use napi::Result;
use napi_derive::napi;

use crate::agent::NapiAgent;

fn failure(error: impl std::fmt::Display) -> Error {
    Error::from_reason(error.to_string())
}
fn auth() -> Result<evot::auth::AuthState> {
    evot::auth::load_auth()
        .map_err(failure)?
        .ok_or_else(|| Error::from_reason("share requires sign-in; run evot login"))
}

#[napi]
impl NapiAgent {
    #[napi]
    pub async fn record_share_notices(
        &self,
        session_id: String,
        notices_json: String,
    ) -> Result<()> {
        let notices = serde_json::from_str(&notices_json).map_err(failure)?;
        evot::share::record_notices(self.agent.storage(), &session_id, notices)
            .await
            .map_err(failure)
    }

    #[napi]
    pub async fn share_session(&self, session_id: String) -> Result<String> {
        let state = auth()?;
        let session = evot::sessions::Session::open(&session_id, self.agent.storage())
            .await
            .map_err(failure)?
            .ok_or_else(|| Error::from_reason("session not found"))?;
        let entries = session.load_all_entries().await.map_err(failure)?;
        if entries.is_empty() {
            return Err(Error::from_reason("nothing to share"));
        }
        let payload =
            evot::share::export_session(&session.meta().await, &entries, env!("CARGO_PKG_VERSION"));
        let result = evot::share::upload(&state, &payload)
            .await
            .map_err(failure)?;
        serde_json::to_string(&result).map_err(failure)
    }

    #[napi]
    pub async fn list_shares(&self) -> Result<String> {
        let value = evot::share::list(&auth()?).await.map_err(failure)?;
        serde_json::to_string(&value).map_err(failure)
    }

    #[napi]
    pub async fn delete_share(&self, id: String) -> Result<()> {
        evot::share::delete(&auth()?, &id).await.map_err(failure)?;
        Ok(())
    }
}
