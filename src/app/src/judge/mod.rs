//! The session judge: TypeSafe Jev published by the cloud catalog as a
//! `role=judge` model, reached through the same gateway as chat models.
//!
//! One construction site, so every consumer (context pruning today, tool-call
//! review or routing later) gets the same endpoint and the same transport.

mod trace;

use std::path::Path;
use std::sync::Arc;

use evot_engine::judge::Judge;
use evot_engine::judge::ProviderJudge;
use evot_engine::provider::AnthropicProvider;
use evot_engine::provider::OpenAiCompatProvider;
use evot_engine::provider::OpenAiResponsesProvider;
use evot_engine::provider::StreamProvider;
pub use trace::TracingJudge;
pub use trace::JUDGE_TRACE_FILE;
pub use trace::JUDGE_TRACE_VERSION;

use crate::conf::resolve_model_config;
use crate::conf::JudgeEndpoint;
use crate::conf::Protocol;

/// The judge as the server publishes it right now. Resolved per run from the
/// synced models cache, so switching the judge model off on the server stops
/// pruning at the next run without a restart.
pub fn current() -> Option<Arc<dyn Judge>> {
    from_endpoint(crate::conf::load::current_judge_endpoint().as_ref())
}

/// The current judge, recording every request to the session's
/// `judge-trace.jsonl` when the session has a directory on disk.
pub fn current_for_session(session_dir: Option<&Path>) -> Option<Arc<dyn Judge>> {
    let judge = current()?;
    Some(match session_dir {
        Some(dir) => Arc::new(TracingJudge::new(judge, dir.to_path_buf())),
        None => judge,
    })
}

/// Build the judge for a published endpoint. `None` when no judge is
/// published, which keeps every judge-driven feature switched off.
pub fn from_endpoint(endpoint: Option<&JudgeEndpoint>) -> Option<Arc<dyn Judge>> {
    let endpoint = endpoint?;
    let provider: Arc<dyn StreamProvider> = match endpoint.protocol {
        Protocol::Anthropic => Arc::new(AnthropicProvider),
        Protocol::OpenAiResponses => Arc::new(OpenAiResponsesProvider),
        Protocol::OpenAi => Arc::new(OpenAiCompatProvider),
    };
    let model_config = resolve_model_config(
        endpoint.protocol.clone(),
        &endpoint.provider,
        &endpoint.model,
        Some(&endpoint.base_url),
        Default::default(),
        Default::default(),
        None,
        None,
        Some(false),
    );
    Some(Arc::new(ProviderJudge::new(
        provider,
        endpoint.model.clone(),
        endpoint.api_key.clone(),
        Some(model_config),
    )))
}
