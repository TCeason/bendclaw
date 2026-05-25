use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use parking_lot::RwLock;

use super::run::control::RunControl;
use super::run::convert;
use super::run::run::Run;
use super::run::runtime;
use super::run::runtime::TurnFactory;
use super::session::Session;
use super::tools::build_tools;
use super::tools::ToolMode;
use super::variables::Variables;
use crate::agent::prompt::Section;
use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::error::EvotError;
use crate::error::Result;
use crate::storage::open_storage;
use crate::storage::MemoryStorage;
use crate::storage::Storage;
use crate::telemetry::config::TelemetryConfig;
use crate::types::GoalStatus;
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
        }
    }

    pub fn with_input(input: Vec<evot_engine::Content>) -> Self {
        Self {
            input,
            session_id: None,
            mode: ToolMode::Headless,
            source: String::new(),
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

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

// ---------------------------------------------------------------------------
// SubmitOutcome — result of a submit: either a Run or a handled command
// ---------------------------------------------------------------------------

pub enum SubmitOutcome {
    /// Normal agent run.
    Run(Run),
    /// A gateway command was handled; carry this text back to the caller.
    Command(String),
    /// A command was handled AND a follow-up run was kicked off
    /// (e.g. `/goal set` injects the continuation prompt). The caller
    /// should display `msg` and then stream `run`.
    CommandThenRun { msg: String, run: Run },
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

const PLANNING_MODE_PROMPT: &str = include_str!("prompt/prompts/plan.md");

struct ActiveRun {
    run_id: String,
    handle: RunControl,
    done: Arc<AtomicBool>,
}

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
    /// session_id → (run_id, handle, done_flag)
    active_runs: Arc<parking_lot::Mutex<HashMap<String, ActiveRun>>>,
    /// OTel telemetry config (endpoint presence = enabled).
    telemetry: crate::telemetry::config::TelemetryConfig,
    /// Holds the OTel exporter alive for the agent's lifetime.
    _telemetry_exporter: Option<crate::telemetry::exporter::TelemetryExporter>,
    /// Session-level task tracking state (TodoWrite).
    todo_meta: super::tools::todo_write::TodoMeta,
}

impl Agent {
    pub fn new(config: &Config, cwd: impl Into<String>) -> Result<Arc<Self>> {
        let cwd = cwd.into();
        let storage = open_storage(&config.storage)?;
        let system_prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        let telemetry_exporter =
            crate::telemetry::exporter::TelemetryExporter::init(&config.telemetry);
        Ok(Arc::new(Self {
            llm: RwLock::new(config.active_llm()?),
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
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            telemetry: config.telemetry.clone(),
            _telemetry_exporter: telemetry_exporter,
            todo_meta: super::tools::todo_write::TodoMeta::new(),
        }))
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

    pub fn with_storage(self: &Arc<Self>, storage: Arc<dyn Storage>) -> Arc<Self> {
        *self.storage.write() = storage;
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

    pub fn limits(&self) -> ExecutionLimits {
        self.limits.read().clone()
    }

    pub fn set_model(&self, model: String) {
        self.llm.write().model = model;
    }

    pub fn set_llm(&self, llm: LlmConfig) {
        *self.llm.write() = llm;
    }

    /// Set the active model by spec (e.g. "deepseek-chat" or "openrouter:google/gemini-2.5-pro").
    /// Resolves provider+model from config. Falls back to just updating the model name.
    pub fn set_model_by_spec(&self, config: &Config, spec: &str) {
        if let Ok((provider_name, model_override)) = config.resolve_model_spec(spec) {
            if let Ok(llm) = config.build_llm(&provider_name, model_override) {
                self.set_llm(llm);
                return;
            }
        }
        self.set_model(spec.to_string());
    }

    /// Switch provider by spec. Unlike `set_model_by_spec`, this fails if the spec
    /// cannot be resolved to a known provider.
    pub fn set_provider_by_spec(&self, config: &Config, spec: &str) -> Result<()> {
        let (provider_name, model_override) = config.resolve_model_spec(spec)?;
        let llm = config.build_llm(&provider_name, model_override)?;
        self.set_llm(llm);
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
            if !ar.done.load(Ordering::Relaxed) {
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
            if !ar.done.load(Ordering::Relaxed) {
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
            if ar.done.load(Ordering::Relaxed) {
                map.remove(session_id);
                return false;
            }
            true
        } else {
            false
        }
    }

    // -- query ---------------------------------------------------------------

    pub async fn submit(self: &Arc<Self>, request: QueryRequest) -> Result<SubmitOutcome> {
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
        // Intercept gateway commands (/clear, /goto, ...)
        if let Some(outcome) = self.maybe_handle_command(&request, &session).await? {
            return Ok(outcome);
        }

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
            Command::Goto(seq) => {
                if !session.is_valid_context_seq(seq).await? {
                    let max = session.max_seq().await;
                    return Ok(Some(SubmitOutcome::Command(format!(
                        "Invalid message number. Only user messages (1-{max}) are valid goto targets."
                    ))));
                }
                let session_id = session.session_id().await;
                self.abort_run(&session_id);
                session.write_goto_marker(seq).await?;
                session.save().await?;
                // Show context window around the goto point
                let entries = session.recent_context_entries(5).await?;
                let mut lines = vec![format!("Moved to message #{seq}.")];
                for (s, item) in &entries {
                    let is_target = *s == seq;
                    let marker = if is_target { " ←" } else { "" };
                    lines.push(format!("  {}{}", format_history_entry(*s, item), marker));
                }
                // If target wasn't in the window (it's now in snapshot with seq=0),
                // show it explicitly
                if !entries.iter().any(|(s, _)| *s == seq) {
                    if let Some(item) = session.get_item_at(seq).await? {
                        lines.push(format!("  target: {} ←", format_history_entry(seq, &item)));
                    }
                }
                Ok(Some(SubmitOutcome::Command(lines.join("\n"))))
            }
            Command::History(limit) => {
                let entries = session.recent_context_entries(usize::MAX).await?;
                let user_entries: Vec<_> = entries
                    .iter()
                    .filter(|(_, item)| {
                        matches!(item, crate::types::TranscriptItem::User { text, .. } if !text.starts_with("[Summary]"))
                    })
                    .collect();
                if user_entries.is_empty() {
                    return Ok(Some(SubmitOutcome::Command(
                        "No messages in session.".into(),
                    )));
                }
                let start = user_entries.len().saturating_sub(limit);
                let mut lines = Vec::new();
                for (seq, item) in &user_entries[start..] {
                    lines.push(format!("  {}", format_history_entry(*seq, item)));
                }
                Ok(Some(SubmitOutcome::Command(lines.join("\n"))))
            }
            Command::Goal(sub) => {
                let command_session = session.clone();
                let run_session = session.clone();
                let agent = Arc::clone(self);
                let ctx = crate::agent::goal::command::GoalCommandContext {
                    goal_verification_enabled: self.sandbox.goal_verification_enabled(),
                    start_run: Box::new(move |request| {
                        let session = run_session.clone();
                        let agent = Arc::clone(&agent);
                        Box::pin(async move { agent.start_run(request, session).await })
                    }),
                };
                Ok(Some(
                    crate::agent::goal::command::handle(
                        command_session.as_ref(),
                        request,
                        sub,
                        ctx,
                    )
                    .await?,
                ))
            }
            Command::Dump { target } => {
                let msg = self
                    .handle_dump_command(&request.mode, session, target.as_deref())
                    .await?;
                Ok(Some(SubmitOutcome::Command(msg)))
            }
        }
    }

    // -- run execution (private) ----------------------------------------------

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

        // Shared done flag — set by on_complete, checked at registration
        let done = Arc::new(AtomicBool::new(false));

        // Build cleanup callback — mark done, remove only if still this run
        let active_runs = self.active_runs.clone();
        let sid = session_id.clone();
        let rid = run_id.clone();
        let done_flag = done.clone();
        let on_complete: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            done_flag.store(true, Ordering::Release);
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
            mode: request.mode.clone(),
            session_id: session_id.clone(),
        });

        let verify_fn =
            crate::agent::goal::verifier_agent::build_verify_fn(&self.llm.read(), &self.cwd);

        let run = runtime::execute_run(runtime::ExecuteRunArgs {
            run_id: run_id.clone(),
            session_id: session_id.clone(),
            session,
            initial_input: request.input,
            factory,
            on_complete: Some(on_complete),
            telemetry: Some(self.telemetry.clone()),
            verify_fn: Some(verify_fn),
        });

        // Register active run — skip if on_complete already fired
        if !done.load(Ordering::Acquire) {
            self.active_runs.lock().insert(session_id, ActiveRun {
                run_id,
                handle: run.handle(),
                done,
            });
        }

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
            active_runs: _,
            telemetry: _,
            _telemetry_exporter: _,
            todo_meta: _,
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
                goal_verification_enabled: sandbox.goal_verification_enabled,
            },
            active_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            telemetry: TelemetryConfig::default(),
            _telemetry_exporter: None,
            todo_meta: super::tools::todo_write::TodoMeta::new(),
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

    pub async fn create_session(&self, source: &str) -> Result<SessionMeta> {
        let model = self.llm.read().model.clone();
        let storage = self.storage.read().clone();
        let id = crate::types::new_id();
        let session =
            Session::new_with_source(id, self.cwd.clone(), model, source, storage).await?;
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

    pub async fn load_session(&self, id: &str) -> Result<Option<Arc<Session>>> {
        let storage = self.storage.read().clone();
        Session::open(id, storage).await
    }

    // -- private -------------------------------------------------------------

    fn build_system_prompt(
        &self,
        mode: &ToolMode,
        _goal: Option<&crate::types::SessionGoal>,
    ) -> (String, Vec<Section>) {
        let mut sections = self.system_prompt_sections.read().clone();

        if matches!(mode, ToolMode::Planning { .. }) {
            sections.push(Section {
                name: "planning_mode",
                text: PLANNING_MODE_PROMPT.to_string(),
            });
        }

        if let Some(vars) = self.variables.read().as_ref() {
            let names = vars.variable_names();
            if !names.is_empty() {
                let text = format!(
                    "Available variables: {}\n\n\
                     These variables are automatically available in all bash commands \
                     as environment variables. Use $VAR_NAME to reference them.\n\
                     Do not print, echo, or expose variable values.",
                    names.join(", ")
                );
                sections.push(Section {
                    name: "variables",
                    text,
                });
            }
        }

        if self.sandbox.enabled {
            sections.push(Section {
                name: "sandbox",
                text: "# Sandbox Mode\n\
                       You are running in a sandboxed environment with OS-level filesystem restrictions.\n\
                       - File access is restricted to the project workspace and explicitly allowed directories.\n\
                       - The user's home directory ($HOME) is NOT accessible except for allowed paths.\n\
                       - Do NOT attempt to install packages (pip install, brew install, curl | sh, etc.) — \
                       they will fail with \"Operation not permitted\".\n\
                       - Do NOT retry commands that fail with permission errors — the restriction is \
                       enforced by the kernel and cannot be bypassed.\n\
                       - Use only tools and binaries already available on PATH."
                    .to_string(),
            });
        }

        let text = sections
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        (text, sections)
    }

    /// Build a structured snapshot of what evot would send to the LLM right
    /// now (system prompt + tool definitions + skill instructions). Persists
    /// to JSON and returns a human-readable status string.
    async fn handle_dump_command(
        self: &Arc<Self>,
        mode: &ToolMode,
        session: &Arc<Session>,
        target: Option<&str>,
    ) -> Result<String> {
        let session_id = session.session_id().await;
        // build_turn runs the full per-turn assembly (tools, skills_dirs,
        // memory tool, goal tools). Reuse it so the dump matches reality.
        let turn = self
            .build_turn(mode, Arc::clone(session), &session_id, Vec::new())
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
        let model = self.llm.read().model.clone();
        let storage = self.storage.read().clone();
        let session = match session_id {
            Some(id) => match Session::open(id, storage.clone()).await? {
                Some(session) => {
                    session.set_model(model).await;
                    session
                }
                None => {
                    Session::new_with_source(
                        id.to_string(),
                        self.cwd.clone(),
                        model,
                        source,
                        storage,
                    )
                    .await?
                }
            },
            None => {
                let id = crate::types::new_id();
                Session::new_with_source(id, self.cwd.clone(), model, source, storage).await?
            }
        };

        if let Some(goal) = session.read_goal().await {
            tracing::info!(
                condition = %goal.condition,
                "goal restored on resume"
            );
        }

        Ok(session)
    }

    async fn build_turn(
        &self,
        mode: &ToolMode,
        session: Arc<Session>,
        session_id: &str,
        input: Vec<evot_engine::Content>,
    ) -> Result<runtime::TurnInput> {
        let llm = self.llm.read().clone();
        let active_goal = session.read_goal().await;
        let (mut system_prompt, mut sections) =
            self.build_system_prompt(mode, active_goal.as_ref());
        let envs = self
            .variables()
            .map(|v| v.all_env_pairs())
            .unwrap_or_default();
        // Build path guard from sandbox policy
        let cwd_path = std::path::Path::new(&self.cwd);
        let memory_dirs = super::prompt::memory::resolve_memory_dirs(&self.cwd);
        let skill_dirs = self.skills_dirs.read().clone();
        let sandbox_rt = self
            .sandbox
            .build_runtime(cwd_path, &memory_dirs, &skill_dirs)?;

        let mut tools = build_tools(
            mode,
            envs,
            sandbox_rt.allow_bash,
            sandbox_rt.bash_sandbox_dirs,
        );

        if !mode.is_readonly()
            && !mode.is_planning()
            && active_goal
                .as_ref()
                .is_some_and(|goal| goal.status == GoalStatus::Active)
        {
            tools.push(Box::new(
                super::goal::update_tasks_tool::UpdateGoalTasksTool::new(Arc::clone(&session)),
            ));
        }

        if !mode.is_readonly() && !mode.is_planning() {
            tools.push(Box::new(super::tools::todo_write::TodoWriteTool::new(
                Arc::clone(&session),
                self.todo_meta.clone(),
            )));
        }

        if !mode.is_readonly() {
            if let Some(mt) = super::prompt::memory::load_memory_tool(&self.cwd) {
                if mode.is_planning() {
                    tools.push(Box::new(mt.disallow_writes(
                        "Not allowed in planning mode. Use /act to switch.",
                    )));
                } else {
                    tools.push(Box::new(mt));
                }
            }
        }

        // Append skills fragment to system prompt so the engine receives it
        // as part of the prompt text (engine no longer mutates system_prompt).
        if let Ok(specs) = crate::agent::prompt::skill::load_skills(&skill_dirs) {
            if !specs.is_empty() {
                let skill_set = evot_engine::SkillSet::new(specs);
                let fragment = skill_set.format_for_prompt();
                if !fragment.is_empty() {
                    system_prompt.push_str("\n\n");
                    system_prompt.push_str(&fragment);
                    sections.push(Section {
                        name: "skills",
                        text: fragment,
                    });
                }
            }
        }

        // Append current TodoWrite tasks to system prompt.
        {
            self.todo_meta.increment_turn();
            let tasks = self.todo_meta.state.lock().await;
            if !tasks.is_empty() {
                let mut fragment = String::from("# Current tasks\n\nThese tasks are already tracked. Only call TodoWrite to change status (e.g. mark completed), not to recreate this list.\n");
                for t in tasks.iter() {
                    let status = match t.status {
                        crate::types::GoalTaskStatus::Pending => "pending",
                        crate::types::GoalTaskStatus::InProgress => "in_progress",
                        crate::types::GoalTaskStatus::Completed => "completed",
                    };
                    fragment.push_str(&format!("\n- [{}] {}", status, t.title));
                }
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&fragment);
                sections.push(Section {
                    name: "tasks",
                    text: fragment,
                });
            } else if self.todo_meta.should_remind_never_used(10) {
                let reminder = "The TodoWrite tool hasn't been used recently. \
                    If you're working on tasks that would benefit from tracking progress, \
                    consider using the TodoWrite tool to track progress. \
                    Only use it if it's relevant to the current work. \
                    This is just a gentle reminder - ignore if not applicable.";
                system_prompt.push_str("\n\n");
                system_prompt.push_str(reminder);
                sections.push(Section {
                    name: "todo_reminder",
                    text: reminder.into(),
                });
            }
            // Stale reminder: used before but not updated recently.
            // Only fire when tasks are empty in system prompt (otherwise the model
            // already sees them and doesn't need a nudge to call TodoWrite).
            if tasks.is_empty() && self.todo_meta.should_remind_stale(25) {
                let reminder = "The TodoWrite tool hasn't been used recently. \
                    If you're working on tasks that would benefit from tracking progress, \
                    consider using the TodoWrite tool to track progress. \
                    Only use it if it's relevant to the current work. \
                    This is just a gentle reminder - ignore if not applicable.";
                system_prompt.push_str("\n\n");
                system_prompt.push_str(reminder);
                sections.push(Section {
                    name: "todo_reminder",
                    text: reminder.into(),
                });
            }
        }

        let prior_transcripts = session.transcript().await;
        let prior_messages = convert::into_agent_messages(&prior_transcripts);
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
                limits: self.limits.read().clone(),
                skills_dirs: skill_dirs,
                tools,
                thinking_level: llm.thinking_level,
                compat_caps: llm.compat_caps,
                cwd: cwd_path.to_path_buf(),
                path_guard: sandbox_rt.path_guard,
                spill_dir: self
                    .spill_root
                    .as_ref()
                    .map(|root| root.join("sessions").join(session_id).join("tool-results")),
                prompt_cache_key: Some(session_id.to_string()),
            },
            history: prior_messages,
            input,
            session,
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
}

#[async_trait::async_trait]
impl TurnFactory for AgentTurnFactory {
    async fn build(&self, input: Vec<evot_engine::Content>) -> Result<runtime::TurnInput> {
        self.agent
            .build_turn(
                &self.mode,
                Arc::clone(&self.session),
                &self.session_id,
                input,
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_history_entry(seq: u64, item: &crate::types::TranscriptItem) -> String {
    let is_user = matches!(item, crate::types::TranscriptItem::User { .. });
    let role = match item {
        crate::types::TranscriptItem::User { .. } => "user",
        crate::types::TranscriptItem::Assistant { .. } => "assistant",
        _ => {
            debug_assert!(false, "history entry must be user or assistant");
            "unknown"
        }
    };
    let preview = crate::types::entry_preview(item);
    if seq == 0 || !is_user {
        format!("  …   {:<10} {}", role, preview)
    } else {
        format!("#{:<4} {:<10} {}", seq, role, preview)
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

fn mode_label(mode: &ToolMode) -> &'static str {
    match mode {
        ToolMode::Interactive { .. } => "Interactive",
        ToolMode::Headless => "Headless",
        ToolMode::Planning { .. } => "Planning",
        ToolMode::Readonly => "Readonly",
    }
}

fn thinking_label(level: evot_engine::ThinkingLevel) -> &'static str {
    match level {
        evot_engine::ThinkingLevel::Off => "off",
        evot_engine::ThinkingLevel::Minimal => "minimal",
        evot_engine::ThinkingLevel::Low => "low",
        evot_engine::ThinkingLevel::Medium => "medium",
        evot_engine::ThinkingLevel::High => "high",
        evot_engine::ThinkingLevel::Adaptive => "adaptive",
    }
}

fn build_prompt_dump(_agent: &Agent, mode: &ToolMode, turn: &runtime::TurnInput) -> PromptDump {
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
            SubmitOutcome::Command(_) | SubmitOutcome::CommandThenRun { .. } => Err(
                EvotError::Run("commands not supported in forked agent".into()),
            ),
        }
    }
}
