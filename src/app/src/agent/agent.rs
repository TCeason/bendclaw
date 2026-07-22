use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use super::run::control::RunControl;
use super::run::run::Run;
use super::run::runtime;
use super::run::runtime::TurnFactory;
use super::session::Session;
use super::tools::build_tools;
use super::tools::HostTools;
use super::tools::ToolMode;
use super::variables::Variables;
use crate::agent::prompt::dynamic_sections;
use crate::agent::prompt::DynamicContext;
use crate::agent::prompt::PromptMode;
use crate::agent::prompt::Section;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::conf::Protocol;
use crate::error::EvotError;
use crate::error::Result;
use crate::storage::open_storage;
use crate::storage::MemoryStorage;
use crate::storage::Storage;
use crate::types::ListSessions;
use crate::types::PromptDump;
use crate::types::SectionDump;
use crate::types::SessionMeta;
use crate::types::SkillInstructionDump;
use crate::types::SystemPromptDump;
use crate::types::TokenTotals;
use crate::types::ToolDump;
use crate::types::TranscriptItem;

// ---------------------------------------------------------------------------
// ExecutionLimits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub max_duration_secs: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 512,
            max_total_tokens: 100_000_000,
            max_duration_secs: 3600,
        }
    }
}

// ---------------------------------------------------------------------------
// QueryRequest
// ---------------------------------------------------------------------------

pub struct QueryRequest {
    pub input: Vec<evot_engine::Content>,
    pub session_id: Option<String>,
    pub mode: ToolMode,
    pub source: String,
    /// Host-owned tools (ask_user, …) to attach to this run. `None` when the
    /// caller has no host bridge (e.g. gateway/headless callers).
    pub host_tools: Option<HostTools>,
}

impl QueryRequest {
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            input: vec![evot_engine::Content::Text {
                text: prompt.into(),
            }],
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
            host_tools: None,
        }
    }

    pub fn with_input(input: Vec<evot_engine::Content>) -> Self {
        Self {
            input,
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
            host_tools: None,
        }
    }

    /// Extract plain text from input content (for transcript, titles, logs).
    pub fn input_text(&self) -> String {
        crate::agent::run::convert::extract_content_text(&self.input)
    }

    pub fn session_id(mut self, id: Option<String>) -> Self {
        self.session_id = id;
        self
    }

    pub fn mode(mut self, mode: ToolMode) -> Self {
        self.mode = mode;
        self
    }

    /// Attach host-owned tools (the host bridge plus its registered specs).
    pub fn host_tools(mut self, host_tools: Option<HostTools>) -> Self {
        self.host_tools = host_tools;
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

/// Expand prompt-rewriting commands (`/mem`, `/mem <terms>`) into normal
/// prompts. Non-command input passes through unchanged.
fn expand_prompt_command(mut request: QueryRequest) -> QueryRequest {
    use crate::gateway::command::memorize_prompt;
    use crate::gateway::command::parse_command;
    use crate::gateway::command::recall_prompt;
    use crate::gateway::command::Command;

    let text = match parse_command(&request.input_text()) {
        Some(Command::Memorize) => memorize_prompt(),
        Some(Command::MemorySearch { query }) => recall_prompt(&query),
        _ => return request,
    };
    request.input = vec![evot_engine::Content::Text { text }];
    request
}

// ---------------------------------------------------------------------------
// SubmitOutcome — result of a submit: either a Run or a handled command
// ---------------------------------------------------------------------------

pub enum SubmitOutcome {
    /// Normal agent run.
    Run(Run),
    /// A gateway command was handled; carry this text back to the caller.
    Command(String),
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

struct ActiveRun {
    run_id: String,
    handle: RunControl,
    completed: tokio_util::sync::CancellationToken,
}

enum AbortRunOutcome {
    Stopped,
    Cancelled,
    TimedOut,
}

const RUN_ABORT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const COMPACTION_SUMMARY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Agent {
    llm: RwLock<LlmConfig>,
    system_prompt: RwLock<String>,
    /// Per-section breakdown matching `system_prompt`. Used by `/_dump`.
    /// Empty when `with_system_prompt` was called with a raw string and no
    /// sections; the dump path then treats the whole prompt as a single
    /// "system_prompt" section.
    system_prompt_sections: RwLock<Vec<Section>>,
    limits: RwLock<ExecutionLimits>,
    skills_dirs: RwLock<Vec<PathBuf>>,
    cwd: String,
    /// Root dir for spill files. Only set when storage backend is Fs.
    spill_root: Option<PathBuf>,
    storage: RwLock<Arc<dyn Storage>>,
    variables: RwLock<Option<Arc<Variables>>>,
    sandbox: super::sandbox::SandboxPolicy,
    provider_override: RwLock<Option<Arc<dyn evot_engine::provider::StreamProvider>>>,
    /// session_id → (run_id, handle, done_flag)
    active_runs: Arc<parking_lot::Mutex<HashMap<String, ActiveRun>>>,
}

impl Agent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Arc<Self>> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        Ok(Arc::new(Self::new_inner(config, cwd, storage)?))
    }

    fn new_inner(config: &Config, cwd: String, storage: Arc<dyn Storage>) -> Result<Self> {
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        Ok(Self {
            llm: RwLock::new(
                config
                    .active_llm()
                    .unwrap_or_else(|_| LlmConfig::unconfigured()),
            ),
            system_prompt: RwLock::new(system_prompt),
            system_prompt_sections: RwLock::new(Vec::new()),
            limits: RwLock::new(ExecutionLimits::default()),
            skills_dirs: RwLock::new(Vec::new()),
            cwd,
            spill_root: match config.storage.backend {
                crate::conf::StorageBackend::Fs => Some(config.storage.fs.root_dir.clone()),
                _ => None,
            },
            storage: RwLock::new(storage),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy::from_config(&config.sandbox),
            provider_override: RwLock::new(None),
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        })
    }

    pub fn new_with_provider_for_test(
        config: &Config,
        cwd: impl Into<String>,
        storage: Arc<dyn Storage>,
        provider: impl evot_engine::provider::StreamProvider + 'static,
    ) -> Result<Arc<Self>> {
        let agent = Arc::new(Self::new_inner(config, cwd.into(), storage)?);
        *agent.provider_override.write() = Some(Arc::new(provider));
        Ok(agent)
    }

    // -- configuration (fluent setters) --------------------------------------

    pub fn with_system_prompt(self: &Arc<Self>, prompt: impl Into<String>) -> Arc<Self> {
        *self.system_prompt.write() = prompt.into();
        self.system_prompt_sections.write().clear();
        Arc::clone(self)
    }

    /// Set the system prompt along with its per-section breakdown. The joined
    /// `text` must equal `sections` joined by `"\n\n"` — same invariant as
    /// `SystemPrompt::build_with_sections`.
    pub fn with_system_prompt_sections(
        self: &Arc<Self>,
        text: String,
        sections: Vec<Section>,
    ) -> Arc<Self> {
        *self.system_prompt.write() = text;
        *self.system_prompt_sections.write() = sections;
        Arc::clone(self)
    }

    pub fn append_system_prompt(self: &Arc<Self>, extra: &str) -> Arc<Self> {
        let mut sp = self.system_prompt.write();
        sp.push('\n');
        sp.push_str(extra);
        drop(sp);
        // Track the appended chunk so /_dump still shows where it came from.
        self.system_prompt_sections.write().push(Section {
            name: "append",
            text: extra.to_string(),
        });
        Arc::clone(self)
    }

    pub fn with_limits(self: &Arc<Self>, limits: ExecutionLimits) -> Arc<Self> {
        *self.limits.write() = limits;
        Arc::clone(self)
    }

    pub fn with_skills_dirs(self: &Arc<Self>, dirs: Vec<PathBuf>) -> Arc<Self> {
        *self.skills_dirs.write() = dirs;
        self.with_claude_skills_dirs()
    }

    fn with_claude_skills_dirs(self: &Arc<Self>) -> Arc<Self> {
        if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            let claude_dir = PathBuf::from(home).join(".claude").join("skills");
            if claude_dir.is_dir() {
                let mut dirs = self.skills_dirs.write();
                if !dirs.contains(&claude_dir) {
                    dirs.push(claude_dir);
                }
            }
        }
        Arc::clone(self)
    }

    pub fn with_variables(self: &Arc<Self>, variables: Arc<Variables>) -> Arc<Self> {
        *self.variables.write() = Some(variables);
        Arc::clone(self)
    }

    // -- getters -------------------------------------------------------------

    pub fn system_prompt(&self) -> String {
        self.system_prompt.read().clone()
    }

    pub fn llm(&self) -> LlmConfig {
        self.llm.read().clone()
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The fully-resolved, ordered list of skills directories the agent scans:
    /// global `~/.evotai/skills`, then config dirs (TOML / env-file /
    /// process-env EVOT_SKILLS_DIRS), then `~/.claude/skills`. This is the
    /// single source of truth the CLI display layer should read so `/skill
    /// list` and the banner never drift from what the agent actually loads.
    pub fn skills_dirs(&self) -> Vec<PathBuf> {
        self.skills_dirs.read().clone()
    }

    pub fn limits(&self) -> ExecutionLimits {
        self.limits.read().clone()
    }

    pub fn set_model(&self, model: String) {
        self.llm.write().model = model;
    }

    pub fn set_llm(&self, llm: LlmConfig) {
        *self.llm.write() = llm;
    }

    /// Set the active thinking level for the current provider.
    pub fn set_thinking_level(&self, level: evot_engine::ThinkingLevel) {
        self.llm.write().thinking_level = level;
    }

    /// Restore a thinking level from its persisted lowercase name (e.g. when
    /// resuming a session). Unknown names and levels the current model does not
    /// support are ignored, leaving the configured default in place.
    pub fn restore_thinking_level(&self, name: &str) {
        let Ok(level) = crate::conf::thinking_level_from_str(name) else {
            return;
        };
        if self.supported_thinking_levels().contains(&level) {
            self.set_thinking_level(level);
        }
    }

    /// Thinking levels the current model can cycle through, in ascending order
    /// of effort. Empty when the model does not honor a reasoning effort (e.g.
    /// an OpenAI-compatible provider without the reasoning-effort capability).
    pub fn supported_thinking_levels(&self) -> Vec<evot_engine::ThinkingLevel> {
        let llm = self.llm.read().clone();
        Self::supported_thinking_levels_for(&llm)
    }

    fn supported_thinking_levels_for(llm: &LlmConfig) -> Vec<evot_engine::ThinkingLevel> {
        let model = super::run::runtime::build_model_config(
            llm.protocol.clone(),
            &llm.provider,
            &llm.model,
            Some(&llm.base_url),
            llm.compat_caps,
            llm.context_window,
            llm.max_tokens,
            llm.supports_image,
        );
        if model.reasoning {
            model.supported_thinking_levels()
        } else {
            Vec::new()
        }
    }

    /// Replace the LLM config while inheriting the session's current thinking
    /// level. Unsupported levels are clamped using pi's ordering: first search
    /// upward from the requested effort, then downward. Models without
    /// selectable reasoning always use `Off`.
    fn set_llm_preserving_thinking(&self, mut llm: LlmConfig) {
        use evot_engine::ThinkingLevel;

        let current = self.llm.read().thinking_level;
        let supported = Self::supported_thinking_levels_for(&llm);
        llm.thinking_level = if supported.is_empty() {
            ThinkingLevel::Off
        } else if current == ThinkingLevel::Adaptive {
            // Adaptive is evot's provider-selected effort. Preserve it across
            // switches between reasoning models; non-reasoning models were
            // handled above.
            ThinkingLevel::Adaptive
        } else if supported.contains(&current) {
            current
        } else {
            const ORDERED: [ThinkingLevel; 7] = [
                ThinkingLevel::Off,
                ThinkingLevel::Minimal,
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
                ThinkingLevel::Xhigh,
                ThinkingLevel::Max,
            ];
            let requested = ORDERED
                .iter()
                .position(|level| *level == current)
                .unwrap_or(0);
            ORDERED[requested..]
                .iter()
                .chain(ORDERED[..requested].iter().rev())
                .find(|level| supported.contains(level))
                .copied()
                .unwrap_or(ThinkingLevel::Off)
        };
        self.set_llm(llm);
    }

    /// The active model's resolved context window in tokens, after applying
    /// explicit overrides. Used to size and validate compaction so the retained
    /// context fits what the model actually accepts.
    pub fn resolved_context_window(&self) -> u32 {
        let llm = self.llm.read();
        super::run::runtime::build_model_config(
            llm.protocol.clone(),
            &llm.provider,
            &llm.model,
            Some(&llm.base_url),
            llm.compat_caps,
            llm.context_window,
            llm.max_tokens,
            llm.supports_image,
        )
        .context_window
    }

    /// Advance the thinking level to the next supported tier, wrapping around.
    /// Returns the new level, or `None` when the model has no selectable levels.
    pub fn cycle_thinking_level(&self) -> Option<evot_engine::ThinkingLevel> {
        let levels = self.supported_thinking_levels();
        if levels.is_empty() {
            return None;
        }
        let current = self.llm.read().thinking_level;
        let next_index = levels
            .iter()
            .position(|l| *l == current)
            .map(|i| (i + 1) % levels.len())
            .unwrap_or(0);
        let next = levels[next_index];
        self.set_thinking_level(next);
        Some(next)
    }

    /// Set the active model by spec (e.g. "deepseek-chat" or "openrouter:google/gemini-2.5-pro").
    ///
    /// Resolution and provider config errors are returned before mutating the
    /// active LLM. Explicit `provider:model` remains the escape hatch for model
    /// ids not listed in config, as long as the provider itself exists.
    pub fn set_model_by_spec(&self, config: &Config, spec: &str) -> Result<()> {
        let (provider_name, model_override) = config.resolve_model_spec(spec)?;
        let llm = config.build_llm(&provider_name, model_override)?;
        self.set_llm_preserving_thinking(llm);
        Ok(())
    }

    /// Switch provider by spec. Unlike `set_model_by_spec`, this fails if the spec
    /// cannot be resolved to a known provider.
    pub fn set_provider_by_spec(&self, config: &Config, spec: &str) -> Result<()> {
        let (provider_name, model_override) = config.resolve_model_spec(spec)?;
        let llm = config.build_llm(&provider_name, model_override)?;
        self.set_llm_preserving_thinking(llm);
        Ok(())
    }

    pub fn variables(&self) -> Option<Arc<Variables>> {
        self.variables.read().clone()
    }

    pub fn storage(&self) -> Arc<dyn Storage> {
        self.storage.read().clone()
    }

    // -- run control ---------------------------------------------------------

    /// Send a steering message to the active run for a session.
    pub fn steer(&self, session_id: &str, input: Vec<evot_engine::Content>) {
        if let Some(ar) = self.active_runs.lock().get(session_id) {
            if !ar.completed.is_cancelled() {
                ar.handle
                    .steer(evot_engine::AgentMessage::Llm(evot_engine::Message::User {
                        content: input,
                        timestamp: evot_engine::now_ms(),
                    }));
            }
        }
    }

    /// Send a follow-up message to the active run for a session.
    pub fn follow_up(&self, session_id: &str, text: impl Into<String>) {
        if let Some(ar) = self.active_runs.lock().get(session_id) {
            if !ar.completed.is_cancelled() {
                ar.handle
                    .follow_up(evot_engine::AgentMessage::Llm(evot_engine::Message::user(
                        text,
                    )));
            }
        }
    }

    /// Abort the active run for a session.
    pub fn abort_run(&self, session_id: &str) {
        if let Some(ar) = self.active_runs.lock().get(session_id) {
            ar.handle.abort();
        }
    }

    /// Check if a session has an active (non-finished) run.
    /// Automatically cleans up finished runs.
    pub fn has_active_run(&self, session_id: &str) -> bool {
        let mut map = self.active_runs.lock();
        if let Some(ar) = map.get(session_id) {
            if ar.completed.is_cancelled() {
                map.remove(session_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    async fn abort_run_and_wait(
        &self,
        session_id: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> AbortRunOutcome {
        let active = {
            let map = self.active_runs.lock();
            map.get(session_id).map(|active| {
                active.handle.abort();
                active.completed.clone()
            })
        };
        let Some(completed) = active else {
            return AbortRunOutcome::Stopped;
        };
        if completed.is_cancelled() {
            return AbortRunOutcome::Stopped;
        }

        tokio::select! {
            _ = cancel.cancelled() => AbortRunOutcome::Cancelled,
            _ = completed.cancelled() => AbortRunOutcome::Stopped,
            _ = tokio::time::sleep(RUN_ABORT_WAIT_TIMEOUT) => {
                tracing::warn!(
                    stage = "compact",
                    status = "run_abort_timeout",
                    session_id = %session_id,
                    timeout_ms = RUN_ABORT_WAIT_TIMEOUT.as_millis() as u64,
                );
                AbortRunOutcome::TimedOut
            }
        }
    }

    /// Manually compact an existing session with an abortable lifecycle.
    pub async fn compact(
        &self,
        session_id: &str,
        custom_instructions: Option<String>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<crate::compact::orchestrator::ManualCompactionOutcome> {
        match self.abort_run_and_wait(session_id, &cancel).await {
            AbortRunOutcome::Stopped => {}
            AbortRunOutcome::Cancelled => {
                return Ok(crate::compact::orchestrator::ManualCompactionOutcome::Cancelled)
            }
            AbortRunOutcome::TimedOut => {
                return Err(EvotError::Run(format!(
                    "active run did not stop within {} seconds; compaction was not started",
                    RUN_ABORT_WAIT_TIMEOUT.as_secs()
                )))
            }
        }
        let Some(session) = self.load_session(session_id).await? else {
            return Ok(crate::compact::orchestrator::ManualCompactionOutcome::NothingToCompact);
        };
        self.compact_resolved_session(&session, custom_instructions, cancel)
            .await
    }

    // -- query ---------------------------------------------------------------

    pub async fn submit(self: &Arc<Self>, request: QueryRequest) -> Result<SubmitOutcome> {
        // Session-independent commands are handled before resolve_session,
        // which would otherwise persist an empty session when the caller has
        // no session yet (e.g. `/resume <query>` from a fresh CLI).
        if let Some(crate::gateway::command::Command::ResumeSearch { query }) =
            crate::gateway::command::parse_command(&request.input_text())
        {
            let msg = self.handle_resume_search(&query).await?;
            return Ok(SubmitOutcome::Command(msg));
        }
        let session = self
            .resolve_session(request.session_id.as_deref(), &request.source)
            .await?;
        self.submit_to_session(request, session).await
    }

    /// Channel path: session is already resolved by the caller (RunManager).
    /// Intercepts gateway commands before starting a run.
    pub async fn submit_to_session(
        self: &Arc<Self>,
        request: QueryRequest,
        session: Arc<Session>,
    ) -> Result<SubmitOutcome> {
        // Intercept gateway commands (/clear, /compact, ...)
        if let Some(outcome) = self.maybe_handle_command(&request, &session).await? {
            return Ok(outcome);
        }
        // Prompt-expanding commands (/mem) rewrite the input and continue as a
        // normal run.
        let request = expand_prompt_command(request);

        let run = self.start_run(request, session).await?;
        Ok(SubmitOutcome::Run(run))
    }

    // -- command handling (private) -------------------------------------------

    async fn maybe_handle_command(
        self: &Arc<Self>,
        request: &QueryRequest,
        session: &Arc<Session>,
    ) -> Result<Option<SubmitOutcome>> {
        use crate::gateway::command::parse_command;
        use crate::gateway::command::Command;

        let cmd = match parse_command(&request.input_text()) {
            Some(cmd) => cmd,
            None => return Ok(None),
        };

        match cmd {
            Command::UsageError(msg) => Ok(Some(SubmitOutcome::Command(msg))),
            Command::Clear => {
                let session_id = session.session_id().await;
                self.abort_run(&session_id);
                session.write_clear_marker().await?;
                session.save().await?;
                Ok(Some(SubmitOutcome::Command("Session cleared.".into())))
            }
            Command::Compact {
                custom_instructions,
            } => {
                let session_id = session.session_id().await;
                let cancel = tokio_util::sync::CancellationToken::new();
                let outcome = match self.abort_run_and_wait(&session_id, &cancel).await {
                    AbortRunOutcome::Stopped => {
                        self.compact_resolved_session(session, custom_instructions, cancel)
                            .await?
                    }
                    AbortRunOutcome::Cancelled => {
                        crate::compact::orchestrator::ManualCompactionOutcome::Cancelled
                    }
                    AbortRunOutcome::TimedOut => {
                        return Err(EvotError::Run(format!(
                            "active run did not stop within {} seconds; compaction was not started",
                            RUN_ABORT_WAIT_TIMEOUT.as_secs()
                        )))
                    }
                };
                let msg = format_manual_compaction_outcome(&outcome);
                Ok(Some(SubmitOutcome::Command(msg)))
            }
            Command::Dump { target } => {
                let msg = self
                    .handle_dump_command(request.mode, session, target.as_deref())
                    .await?;
                Ok(Some(SubmitOutcome::Command(msg)))
            }
            // Expanded into normal prompts by `expand_prompt_command` after
            // this interception step; nothing to handle here.
            Command::Memorize | Command::MemorySearch { .. } => Ok(None),
            // Semantic session search — one-shot LLM ranking, no agent run.
            Command::ResumeSearch { query } => {
                let msg = self.handle_resume_search(&query).await?;
                Ok(Some(SubmitOutcome::Command(msg)))
            }
        }
    }

    // -- run execution (private) ----------------------------------------------

    async fn compact_resolved_session(
        &self,
        session: &Arc<Session>,
        custom_instructions: Option<String>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<crate::compact::orchestrator::ManualCompactionOutcome> {
        if cancel.is_cancelled() {
            return Ok(crate::compact::orchestrator::ManualCompactionOutcome::Cancelled);
        }
        let context_window = self.resolved_context_window() as usize;
        let request = crate::compact::orchestrator::ManualCompactRequest {
            reason: crate::types::CompactReason::Manual,
            custom_instructions,
            summary_override: None,
            summarizer: Some(self.compact_summarizer()),
            settings: crate::compact::orchestrator::CompactSettings {
                context_window,
                ..Default::default()
            },
        };
        let result = crate::compact::orchestrator::compact_session_with_status(
            session,
            request,
            cancel.clone(),
        )
        .await?;
        if result.status == crate::compact::orchestrator::CompactSessionStatus::Cancelled {
            return Ok(crate::compact::orchestrator::ManualCompactionOutcome::Cancelled);
        }
        session.save().await?;
        match result.item {
            Some(crate::types::TranscriptItem::Compact {
                summary,
                tokens_before,
                tokens_after,
                messages_before,
                messages_after,
                ..
            }) => Ok(
                crate::compact::orchestrator::ManualCompactionOutcome::Compacted {
                    summary,
                    tokens_before,
                    tokens_after,
                    messages_before,
                    messages_after,
                    context_window,
                    used_fallback: result.used_fallback,
                },
            ),
            _ => Ok(crate::compact::orchestrator::ManualCompactionOutcome::NothingToCompact),
        }
    }

    fn llm_provider(&self, protocol: &Protocol) -> Arc<dyn evot_engine::provider::StreamProvider> {
        use evot_engine::provider::AnthropicProvider;
        use evot_engine::provider::OpenAiCompatProvider;
        use evot_engine::provider::OpenAiResponsesProvider;

        self.provider_override
            .read()
            .clone()
            .unwrap_or_else(|| match protocol {
                Protocol::Anthropic => Arc::new(AnthropicProvider),
                Protocol::OpenAiResponses => Arc::new(OpenAiResponsesProvider),
                Protocol::OpenAi => Arc::new(OpenAiCompatProvider),
            })
    }

    fn compact_summarizer(&self) -> crate::compact::orchestrator::CompactSummarizer {
        let llm = self.llm.read().clone();
        let provider = self.llm_provider(&llm.protocol);
        crate::compact::orchestrator::CompactSummarizer {
            provider,
            llm,
            max_tokens: 4096,
            timeout: COMPACTION_SUMMARY_TIMEOUT,
        }
    }

    async fn start_run(
        self: &Arc<Self>,
        request: QueryRequest,
        session: Arc<Session>,
    ) -> Result<Run> {
        let session_id = session.meta().await.session_id.clone();
        let run_id = crate::types::new_id();

        // Session-level safety net: abort any existing active run for this session.
        // This ensures no two runs overlap on the same session, regardless of caller
        // (RunManager, HTTP, NAPI). Long-term this could be consolidated into a
        // single coordination layer if all entry points go through RunManager.
        if let Some(ar) = self.active_runs.lock().remove(&session_id) {
            ar.handle.abort();
        }

        tracing::info!(
            stage = "run",
            status = "started",
            run_id = %run_id,
            session_id = %session_id,
            provider = ?self.llm.read().provider,
            model = %self.llm.read().model,
        );

        // Completion is a one-shot signal, not a polled flag. This avoids a
        // manual compaction waiting forever on stale run state.
        let completed = tokio_util::sync::CancellationToken::new();

        // Build cleanup callback — signal completion, remove only if still this run
        let active_runs = self.active_runs.clone();
        let sid = session_id.clone();
        let rid = run_id.clone();
        let completed_signal = completed.clone();
        let on_complete: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            completed_signal.cancel();
            let mut map = active_runs.lock();
            if let Some(ar) = map.get(&sid) {
                if ar.run_id == rid {
                    map.remove(&sid);
                }
            }
        });

        let factory: Arc<dyn TurnFactory> = Arc::new(AgentTurnFactory {
            agent: Arc::clone(self),
            session: Arc::clone(&session),
            mode: request.mode,
            session_id: session_id.clone(),
            host_tools: request.host_tools.clone(),
        });

        let run = runtime::execute_run(runtime::ExecuteRunArgs {
            run_id: run_id.clone(),
            session_id: session_id.clone(),
            session,
            initial_input: request.input,
            factory,
            on_complete: Some(on_complete),
        });

        // Register while holding the same map lock used by on_complete. The
        // completion token is cancelled before that callback takes the lock, so
        // this ordering closes the check/insert race that could leave a finished
        // run registered forever.
        let mut active_runs = self.active_runs.lock();
        if !completed.is_cancelled() {
            active_runs.insert(session_id, ActiveRun {
                run_id,
                handle: run.handle(),
                completed,
            });
        }
        drop(active_runs);

        Ok(run)
    }

    // -- fork ----------------------------------------------------------------

    /// Fork an independent, non-persisted agent for side conversations.
    pub fn fork(self: &Arc<Self>, request: ForkRequest) -> Result<ForkedAgent> {
        let Self {
            llm,
            system_prompt: _,
            system_prompt_sections: _,
            limits,
            skills_dirs: _,
            cwd,
            spill_root: _,
            storage: _,
            variables: _,
            sandbox,
            provider_override: _,
            active_runs: _,
        } = self.as_ref();

        let forked = Arc::new(Self {
            llm: RwLock::new(llm.read().clone()),
            system_prompt: RwLock::new(request.system_prompt),
            system_prompt_sections: RwLock::new(Vec::new()),
            limits: RwLock::new(limits.read().clone()),
            skills_dirs: RwLock::new(vec![]),
            cwd: cwd.clone(),
            spill_root: None,
            storage: RwLock::new(Arc::new(MemoryStorage::new())),
            variables: RwLock::new(None),
            sandbox: super::sandbox::SandboxPolicy {
                enabled: sandbox.enabled,
                extra_dirs: sandbox.extra_dirs.clone(),
            },
            provider_override: RwLock::new(None),
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
        });
        Ok(ForkedAgent {
            agent: forked,
            session_id: None,
        })
    }

    // -- session queries -----------------------------------------------------

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        let storage = self.storage.read().clone();
        storage.list_sessions(ListSessions { limit }).await
    }

    pub async fn list_sessions_with_text(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::search::SessionWithText>> {
        let storage = self.storage.read().clone();
        storage.list_sessions_with_text(limit).await
    }

    pub async fn find_session(&self, id: &str) -> Result<Option<SessionMeta>> {
        let storage = self.storage.read().clone();
        storage.get_session(id).await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let storage = self.storage.read().clone();
        storage.delete_session(session_id).await
    }

    pub async fn list_favorites(&self) -> Result<Vec<String>> {
        let storage = self.storage.read().clone();
        storage.load_favorites().await
    }

    /// Remove deleted ids from the favorites document. Returns how many favorite
    /// entries were pruned.
    pub async fn remove_favorites(&self, session_ids: &[String]) -> Result<usize> {
        let storage = self.storage.read().clone();
        let ids = storage.load_favorites().await?;
        let before = ids.len();
        let kept: Vec<String> = ids
            .into_iter()
            .filter(|id| !session_ids.iter().any(|deleted| deleted == id))
            .collect();
        let removed = before.saturating_sub(kept.len());
        if removed > 0 {
            storage.save_favorites(kept).await?;
        }
        Ok(removed)
    }

    /// Toggle a session's favorite state, returning the new state (`true` =
    /// now favorited). Persisted via the storage backend's favorites document.
    pub async fn toggle_favorite(&self, session_id: &str) -> Result<bool> {
        let storage = self.storage.read().clone();
        let mut ids = storage.load_favorites().await?;
        let now_favorited = if let Some(pos) = ids.iter().position(|id| id == session_id) {
            ids.remove(pos);
            false
        } else {
            ids.push(session_id.to_string());
            true
        };
        storage.save_favorites(ids).await?;
        Ok(now_favorited)
    }

    pub async fn create_session(&self, source: &str) -> Result<SessionMeta> {
        let (provider, model) = {
            let llm = self.llm.read();
            (llm.provider.clone(), llm.model.clone())
        };
        let storage = self.storage.read().clone();
        let id = crate::types::new_id();
        let session = Session::new_with_provider_source(
            id,
            self.cwd.clone(),
            provider,
            model,
            source,
            storage,
        )
        .await?;
        Ok(session.meta().await)
    }

    pub async fn load_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        let storage = self.storage.read().clone();
        match Session::open(id, storage).await? {
            Some(session) => {
                let entries = session.load_all_entries().await?;
                Ok(entries.into_iter().map(|e| e.item).collect())
            }
            None => Ok(Vec::new()),
        }
    }

    pub async fn load_context_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        let storage = self.storage.read().clone();
        match Session::open(id, storage).await? {
            Some(session) => Ok(session.transcript().await),
            None => Ok(Vec::new()),
        }
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<Arc<Session>>> {
        let storage = self.storage.read().clone();
        Session::open(id, storage).await
    }

    // -- private -------------------------------------------------------------

    fn build_system_prompt(&self, mode: ToolMode) -> (String, Vec<Section>) {
        let mut sections = self.system_prompt_sections.read().clone();

        let ctx = DynamicContext {
            mode: prompt_mode(mode),
            sandbox: self.sandbox.enabled,
            variables: self
                .variables
                .read()
                .as_ref()
                .map(|v| v.variable_names())
                .unwrap_or_default(),
        };
        sections.extend(dynamic_sections(&ctx));

        let text = sections
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        (text, sections)
    }

    /// Rank recent sessions against `query` with a one-shot LLM call and
    /// return a human-readable result list (backs the hidden `/_rsearch`
    /// command used by `/resume <query>`).
    async fn handle_resume_search(&self, query: &str) -> Result<String> {
        let storage = self.storage.read().clone();
        let sessions = storage
            .list_sessions_with_text(crate::agent::resume_search::SESSION_LIMIT)
            .await?;
        let llm = self.llm.read().clone();
        if llm.provider.is_empty() || llm.api_key.trim().is_empty() {
            return Err(EvotError::Conf(
                "Semantic session search needs a configured LLM provider.".to_string(),
            ));
        }
        let ctx = crate::agent::resume_search::RankContext {
            provider: self.llm_provider(&llm.protocol),
            llm,
        };
        crate::agent::resume_search::rank_sessions(&ctx, query, &sessions).await
    }

    /// Build a structured snapshot of what evot would send to the LLM right
    /// now (system prompt + tool definitions + skill instructions). Persists
    /// to JSON and returns a human-readable status string.
    async fn handle_dump_command(
        self: &Arc<Self>,
        mode: ToolMode,
        session: &Arc<Session>,
        target: Option<&str>,
    ) -> Result<String> {
        let session_id = session.session_id().await;
        // build_turn runs the full per-turn assembly (tools, skills_dirs,
        // memory tool). Reuse it so the dump matches reality. The dump path has
        // no host bridge, so host tools are omitted — it reflects built-ins.
        let turn = self
            .build_turn(mode, Arc::clone(session), &session_id, Vec::new(), None)
            .await?;

        let dump = build_prompt_dump(self, mode, &turn);

        let path = resolve_dump_path(target)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                EvotError::Agent(format!(
                    "failed to create dump dir {}: {err}",
                    parent.display()
                ))
            })?;
        }
        let json = serde_json::to_string_pretty(&dump)
            .map_err(|err| EvotError::Agent(format!("failed to serialize prompt dump: {err}")))?;
        std::fs::write(&path, json).map_err(|err| {
            EvotError::Agent(format!("failed to write dump to {}: {err}", path.display()))
        })?;

        Ok(format!(
            "Prompt dump saved to {}\n  system_prompt: {} tokens ({} sections)\n  tools: {} entries, {} tokens\n  skills: {} entries, {} tokens\n  total: {} tokens",
            path.display(),
            dump.totals.system_prompt_tokens,
            dump.system_prompt.sections.len(),
            dump.tools.len(),
            dump.totals.tool_definition_tokens,
            dump.skill_instructions.len(),
            dump.totals.skill_instructions_tokens,
            dump.totals.grand_total,
        ))
    }

    async fn resolve_session(
        &self,
        session_id: Option<&str>,
        source: &str,
    ) -> Result<Arc<Session>> {
        let (provider, model) = {
            let llm = self.llm.read();
            (llm.provider.clone(), llm.model.clone())
        };
        let thinking_level = self.persisted_thinking_level();
        let storage = self.storage.read().clone();
        let session = match session_id {
            Some(id) => match Session::open(id, storage.clone()).await? {
                Some(session) => {
                    session.set_model_selection(provider, model).await?;
                    session
                }
                None => {
                    Session::new_with_provider_source(
                        id.to_string(),
                        self.cwd.clone(),
                        provider,
                        model,
                        source,
                        storage,
                    )
                    .await?
                }
            },
            None => {
                let id = crate::types::new_id();
                Session::new_with_provider_source(
                    id,
                    self.cwd.clone(),
                    provider,
                    model,
                    source,
                    storage,
                )
                .await?
            }
        };
        // Mirror the live model selection: every run stamps the session with the
        // agent's current reasoning effort so it survives restarts (persisted by
        // the run's final `save()`).
        session.set_thinking_level(thinking_level).await;

        Ok(session)
    }

    /// The session-facing label for the agent's current thinking level, or
    /// `None` when the level is not a selectable tier for the active model
    /// (e.g. the `Adaptive` default, or a config-set level the model rejects).
    /// Gating on membership keeps persistence symmetric with
    /// [`Self::restore_thinking_level`]: only values that can be restored are
    /// ever written, so the session never carries inert data.
    fn persisted_thinking_level(&self) -> Option<String> {
        let level = self.llm.read().thinking_level;
        if self.supported_thinking_levels().contains(&level) {
            Some(level.as_str().to_string())
        } else {
            None
        }
    }

    async fn build_turn(
        &self,
        mode: ToolMode,
        session: Arc<Session>,
        session_id: &str,
        input: Vec<evot_engine::Content>,
        host_tools: Option<HostTools>,
    ) -> Result<runtime::TurnInput> {
        let llm = self.llm.read().clone();
        if llm.provider.is_empty() {
            return Err(EvotError::Conf(
                "No LLM provider configured. Add one in the dashboard settings \
                 or set EVOT_LLM_PROVIDER and the matching EVOT_LLM_*_API_KEY \
                 in your env file."
                    .to_string(),
            ));
        }
        if llm.api_key.trim().is_empty() {
            return Err(EvotError::Conf(format!(
                "No API key set for provider '{}'. Add it in the dashboard settings \
                 or set EVOT_LLM_{}_API_KEY in your env file.",
                llm.provider,
                llm.provider.to_uppercase().replace('-', "_"),
            )));
        }
        let (system_prompt, sections) = self.build_system_prompt(mode);
        let envs = self
            .variables()
            .map(|v| v.all_env_pairs())
            .unwrap_or_default();
        // Build path guard from sandbox policy. System dirs cover skill scan
        // directories plus the memory vault used by the builtin memory skill.
        let cwd_path = std::path::Path::new(&self.cwd);
        let skill_dirs = self.skills_dirs.read().clone();
        let mut system_dirs = skill_dirs.clone();
        if let Ok(memory_dir) = crate::conf::paths::memory_dir() {
            if let Err(e) = std::fs::create_dir_all(&memory_dir) {
                tracing::warn!("cannot create memory dir {}: {e}", memory_dir.display());
            }
            system_dirs.push(memory_dir);
        }
        let sandbox_rt = self.sandbox.build_runtime(cwd_path, &system_dirs)?;

        let tools = build_tools(
            mode,
            envs,
            sandbox_rt.allow_bash,
            sandbox_rt.bash_sandbox_dirs,
            host_tools,
        );

        // Skill availability is surfaced via the Skill tool's own description,
        // not injected into the system prompt. This keeps the prompt the engine
        // sends exactly what the caller built (aligned with the pi harness).

        // No longer need turn tracking — engine handles it.

        let (prior_messages, compaction_state, transcript_seq) = session.context_snapshot().await;
        let prior_messages = evot_engine::sanitize_tool_pairs(prior_messages);

        Ok(runtime::TurnInput {
            options: runtime::EngineOptions {
                provider: llm.provider,
                protocol: llm.protocol,
                model: llm.model,
                api_key: llm.api_key,
                base_url: Some(llm.base_url),
                system_prompt,
                system_prompt_sections: sections,
                limits: if mode.is_interactive() {
                    None
                } else {
                    Some(self.limits.read().clone())
                },
                skills_dirs: skill_dirs,
                tools,
                thinking_level: llm.thinking_level,
                compat_caps: llm.compat_caps,
                context_window: llm.context_window,
                max_tokens: llm.max_tokens,
                supports_image: llm.supports_image,
                cwd: cwd_path.to_path_buf(),
                path_guard: sandbox_rt.path_guard,
                spill_dir: self
                    .spill_root
                    .as_ref()
                    .map(|root| root.join("sessions").join(session_id).join("tool-results")),
                prompt_cache_key: Some(session_id.to_string()),
                provider_override: self.provider_override.read().clone(),
                compaction_state,
            },
            history: prior_messages,
            input,
            session,
            transcript_seq,
        })
    }
}

// ---------------------------------------------------------------------------
// AgentTurnFactory — bridges Agent's per-turn build to the runtime
// ---------------------------------------------------------------------------

struct AgentTurnFactory {
    agent: Arc<Agent>,
    session: Arc<Session>,
    mode: ToolMode,
    session_id: String,
    host_tools: Option<HostTools>,
}

#[async_trait::async_trait]
impl TurnFactory for AgentTurnFactory {
    async fn build(&self, input: Vec<evot_engine::Content>) -> Result<runtime::TurnInput> {
        self.agent
            .build_turn(
                self.mode,
                Arc::clone(&self.session),
                &self.session_id,
                input,
                self.host_tools.clone(),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_manual_compaction_outcome(
    outcome: &crate::compact::orchestrator::ManualCompactionOutcome,
) -> String {
    match outcome {
        crate::compact::orchestrator::ManualCompactionOutcome::Compacted {
            tokens_before,
            tokens_after,
            messages_before,
            messages_after,
            context_window,
            used_fallback,
            ..
        } => {
            let mut line = format!(
                "Session compacted: {tokens_before} → {tokens_after} tokens, {messages_before} → {messages_after} messages."
            );
            if *used_fallback {
                line.push_str(
                    "\nNote: the LLM summary was unavailable; a deterministic fallback summary was used.",
                );
            }
            if *context_window > 0 && tokens_after >= context_window {
                line.push_str(&format!(
                    "\nWarning: context is still {tokens_after} tokens, above this model's {context_window}-token window. \
                     Switch to a larger-context model or start a new session to continue."
                ));
            }
            line
        }
        crate::compact::orchestrator::ManualCompactionOutcome::NothingToCompact => {
            "Nothing to compact.".into()
        }
        crate::compact::orchestrator::ManualCompactionOutcome::Cancelled => {
            "Compaction cancelled.".into()
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt dump helpers
// ---------------------------------------------------------------------------

/// Conservative whitespace-based proxy for token count. Avoids a tokenizer
/// dependency in the dump path — for prompt-budget sanity checks it's fine,
/// and replay tooling can re-tokenize the text directly. Roughly
/// `len / 4` is the rule of thumb.
fn rough_tokens(s: &str) -> usize {
    let chars = s.chars().count();
    chars.div_ceil(4)
}

fn mode_label(mode: ToolMode) -> &'static str {
    match mode {
        ToolMode::Interactive => "Interactive",
        ToolMode::Headless => "Headless",
        ToolMode::Planning => "Planning",
        ToolMode::Readonly => "Readonly",
    }
}

/// Distil the runtime [`ToolMode`] into the prompt-layer [`PromptMode`].
fn prompt_mode(mode: ToolMode) -> PromptMode {
    match mode {
        ToolMode::Interactive => PromptMode::Interactive,
        ToolMode::Planning => PromptMode::Planning,
        ToolMode::Headless => PromptMode::Headless,
        ToolMode::Readonly => PromptMode::Readonly,
    }
}

fn thinking_label(level: evot_engine::ThinkingLevel) -> &'static str {
    match level {
        evot_engine::ThinkingLevel::Off => "off",
        evot_engine::ThinkingLevel::Minimal => "minimal",
        evot_engine::ThinkingLevel::Low => "low",
        evot_engine::ThinkingLevel::Medium => "medium",
        evot_engine::ThinkingLevel::High => "high",
        evot_engine::ThinkingLevel::Xhigh => "xhigh",
        evot_engine::ThinkingLevel::Max => "max",
        evot_engine::ThinkingLevel::Adaptive => "adaptive",
    }
}

fn build_prompt_dump(_agent: &Agent, mode: ToolMode, turn: &runtime::TurnInput) -> PromptDump {
    let opts = &turn.options;

    // System prompt sections — sourced from the turn (includes planning,
    // variables, sandbox, skills). Falls back to a single section if empty.
    let section_dumps = if opts.system_prompt_sections.is_empty() {
        vec![SectionDump {
            name: "system_prompt".into(),
            text: opts.system_prompt.clone(),
            tokens: rough_tokens(&opts.system_prompt),
        }]
    } else {
        opts.system_prompt_sections
            .iter()
            .map(|s| SectionDump {
                name: s.name.to_string(),
                text: s.text.clone(),
                tokens: rough_tokens(&s.text),
            })
            .collect()
    };

    let system_tokens = rough_tokens(&opts.system_prompt);
    let system_prompt = SystemPromptDump {
        text: opts.system_prompt.clone(),
        tokens: system_tokens,
        sections: section_dumps,
    };

    // Tool definitions
    let mut tool_dumps: Vec<ToolDump> = opts
        .tools
        .iter()
        .map(|t| {
            let name = t.name().to_string();
            let description = t.description().to_string();
            let parameters = t.parameters_schema();
            let serialized = format!("{name}\n{description}\n{parameters}");
            ToolDump {
                name,
                description,
                parameters,
                tokens: rough_tokens(&serialized),
            }
        })
        .collect();
    tool_dumps.sort_by(|a, b| a.name.cmp(&b.name));
    let tool_tokens: usize = tool_dumps.iter().map(|t| t.tokens).sum();

    // Skill instructions — loaded the same way the runtime would.
    let mut skill_instructions = std::collections::BTreeMap::new();
    if !opts.skills_dirs.is_empty() {
        match crate::agent::prompt::skill::load_skills(&opts.skills_dirs) {
            Ok(specs) => {
                for spec in specs {
                    let combined =
                        format!("{}\n{}\n{}", spec.name, spec.description, spec.instructions);
                    skill_instructions.insert(spec.name.clone(), SkillInstructionDump {
                        description: spec.description,
                        instructions: spec.instructions,
                        tokens: rough_tokens(&combined),
                    });
                }
            }
            Err(err) => {
                tracing::warn!("dump: failed to load skills: {err}");
            }
        }
    }
    let skill_tokens: usize = skill_instructions.values().map(|s| s.tokens).sum();

    PromptDump {
        evot_version: env!("CARGO_PKG_VERSION").to_string(),
        cwd: opts.cwd.display().to_string(),
        mode: mode_label(mode).into(),
        model: opts.model.clone(),
        thinking_level: thinking_label(opts.thinking_level).into(),
        system_prompt,
        tools: tool_dumps,
        skill_instructions,
        totals: TokenTotals {
            system_prompt_tokens: system_tokens,
            tool_definition_tokens: tool_tokens,
            skill_instructions_tokens: skill_tokens,
            grand_total: system_tokens + tool_tokens + skill_tokens,
        },
    }
}

fn resolve_dump_path(target: Option<&str>) -> Result<PathBuf> {
    if let Some(t) = target {
        return Ok(PathBuf::from(t));
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| EvotError::Agent("HOME not set; cannot pick default dump path".into()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    Ok(PathBuf::from(home)
        .join(".evotai")
        .join("dumps")
        .join(format!("prompt-{stamp}.json")))
}

// ---------------------------------------------------------------------------
// ForkRequest / ForkedAgent
// ---------------------------------------------------------------------------

pub struct ForkRequest {
    pub system_prompt: String,
}

/// Handle for a forked conversation.
///
/// Wraps an ephemeral `Agent` backed by `MemoryStorage`. Multi-turn context
/// is maintained in-memory by `Session`. Drop to discard — nothing is persisted.
pub struct ForkedAgent {
    agent: Arc<Agent>,
    session_id: Option<String>,
}

impl ForkedAgent {
    pub async fn query(&mut self, prompt: &str) -> Result<Run> {
        let request = QueryRequest::text(prompt)
            .session_id(self.session_id.clone())
            .mode(ToolMode::Readonly);
        let outcome = self.agent.submit(request).await?;
        match outcome {
            SubmitOutcome::Run(run) => {
                if self.session_id.is_none() {
                    self.session_id = Some(run.session_id.clone());
                }
                Ok(run)
            }
            SubmitOutcome::Command(_) => Err(EvotError::Run(
                "commands not supported in forked agent".into(),
            )),
        }
    }
}
