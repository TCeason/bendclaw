//! Stateful Agent struct — wraps the agent loop with state management,
//! steering/follow-up queues, and abort support.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_loop::agent_loop;
use crate::agent_loop::agent_loop_continue;
use crate::agent_loop::AfterTurnFn;
use crate::agent_loop::AgentLoopConfig;
use crate::agent_loop::BeforeTurnFn;
use crate::context::CompactionStrategy;
use crate::context::ContextConfig;
use crate::context::ExecutionLimits;
use crate::provider::ModelConfig;
use crate::provider::StreamProvider;
use crate::tools::guard::PathGuard;
use crate::types::*;

// ---------------------------------------------------------------------------
// RunHandle — cloneable control handle for a single run
// ---------------------------------------------------------------------------

/// Cloneable control handle for a running agent loop.
///
/// `Run` (app layer) owns the event stream; `RunHandle` is the control plane.
/// Clone it freely to steer / follow-up / abort from any thread.
#[derive(Clone)]
pub struct RunHandle {
    steering_queue: Arc<Mutex<Vec<AgentMessage>>>,
    follow_up_queue: Arc<Mutex<Vec<AgentMessage>>>,
    cancel: CancellationToken,
}

impl RunHandle {
    /// Queue a steering message (interrupts agent mid-tool-execution).
    pub fn steer(&self, msg: AgentMessage) {
        self.steering_queue.lock().push(msg);
    }

    /// Queue a follow-up message (processed after agent finishes current turn).
    pub fn follow_up(&self, msg: AgentMessage) {
        self.follow_up_queue.lock().push(msg);
    }

    /// Clear all queued steering messages.
    pub fn clear_steering(&self) {
        self.steering_queue.lock().clear();
    }

    /// Clear all queued follow-up messages.
    pub fn clear_follow_up(&self) {
        self.follow_up_queue.lock().clear();
    }

    /// Abort the run.
    pub fn abort(&self) {
        self.cancel.cancel();
    }

    /// Check if the run has been aborted.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Create a no-op handle (for tests).
    pub fn noop() -> Self {
        Self {
            steering_queue: Arc::new(Mutex::new(Vec::new())),
            follow_up_queue: Arc::new(Mutex::new(Vec::new())),
            cancel: CancellationToken::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// QueueMode
// ---------------------------------------------------------------------------

/// Queue mode for steering and follow-up messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMode {
    /// Deliver one message per turn
    OneAtATime,
    /// Deliver all queued messages at once
    All,
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

/// The main Agent. Owns state, tools, and provider.
pub struct Agent {
    // State
    pub system_prompt: String,
    pub model: String,
    pub api_key: String,
    pub thinking_level: ThinkingLevel,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    model_config: Option<ModelConfig>,
    messages: Vec<AgentMessage>,
    tools: Vec<Box<dyn AgentTool>>,
    provider: Arc<dyn StreamProvider>,

    // Sandbox
    cwd: PathBuf,
    path_guard: Arc<PathGuard>,

    // Queues (shared with the loop via Arc<Mutex>)
    steering_queue: Arc<Mutex<Vec<AgentMessage>>>,
    follow_up_queue: Arc<Mutex<Vec<AgentMessage>>>,
    steering_mode: QueueMode,
    follow_up_mode: QueueMode,

    // Context, limits & caching
    pub context_config: Option<ContextConfig>,
    context_management_disabled: bool,
    pub execution_limits: Option<ExecutionLimits>,
    pub cache_config: CacheConfig,
    pub tool_execution: ToolExecutionStrategy,
    pub retry_policy: crate::retry::RetryPolicy,

    // Lifecycle callbacks
    before_turn: Option<BeforeTurnFn>,
    after_turn: Option<AfterTurnFn>,

    // Input filters
    input_filters: Vec<Arc<dyn InputFilter>>,

    // Custom compaction strategy
    compaction_strategy: Option<Arc<dyn CompactionStrategy>>,

    // Control
    cancel: Option<CancellationToken>,
    is_streaming: bool,

    // Last run handle (for convenience methods on Agent)
    last_run_handle: Option<RunHandle>,

    // Pending completion from a spawned agent loop
    #[allow(clippy::type_complexity)]
    pending_completion: Option<JoinHandle<(Vec<Box<dyn AgentTool>>, Vec<AgentMessage>)>>,
}

impl Agent {
    pub fn new(provider: impl StreamProvider + 'static) -> Self {
        Self {
            system_prompt: String::new(),
            model: String::new(),
            api_key: String::new(),
            thinking_level: ThinkingLevel::Off,
            max_tokens: None,
            temperature: None,
            model_config: None,
            messages: Vec::new(),
            tools: Vec::new(),
            provider: Arc::new(provider),
            cwd: PathBuf::new(),
            path_guard: Arc::new(PathGuard::open()),
            steering_queue: Arc::new(Mutex::new(Vec::new())),
            follow_up_queue: Arc::new(Mutex::new(Vec::new())),
            steering_mode: QueueMode::OneAtATime,
            follow_up_mode: QueueMode::OneAtATime,
            context_config: None,
            context_management_disabled: false,
            execution_limits: Some(ExecutionLimits::default()),
            cache_config: CacheConfig::default(),
            tool_execution: ToolExecutionStrategy::default(),
            retry_policy: crate::retry::RetryPolicy::default(),
            before_turn: None,
            after_turn: None,
            input_filters: Vec::new(),
            compaction_strategy: None,
            cancel: None,
            is_streaming: false,
            last_run_handle: None,
            pending_completion: None,
        }
    }

    // -- Builder-style setters --

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = key.into();
        self
    }

    pub fn with_thinking(mut self, level: ThinkingLevel) -> Self {
        self.thinking_level = level;
        self
    }

    pub fn with_tools(mut self, tools: Vec<Box<dyn AgentTool>>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    pub fn with_path_guard(mut self, guard: Arc<PathGuard>) -> Self {
        self.path_guard = guard;
        self
    }

    pub fn with_model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = Some(config);
        self
    }

    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    pub fn with_context_config(mut self, config: ContextConfig) -> Self {
        self.context_config = Some(config);
        self
    }

    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.cache_config = config;
        self
    }

    pub fn with_tool_execution(mut self, strategy: ToolExecutionStrategy) -> Self {
        self.tool_execution = strategy;
        self
    }

    pub fn with_retry_policy(mut self, policy: crate::retry::RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_retry_disabled(mut self) -> Self {
        self.retry_policy = crate::retry::RetryPolicy::disabled();
        self
    }

    pub fn with_max_retries(mut self, n: usize) -> Self {
        self.retry_policy = crate::retry::RetryPolicy::new(n);
        self
    }

    /// Load skills and register the skill tool.
    ///
    /// Appends the skills index to the system prompt (XML per the
    /// [AgentSkills standard](https://agentskills.io)) and registers a
    /// `SkillTool` so the LLM can activate skills by name.
    ///
    /// **Must be called after `with_tools()`** — `with_tools()` replaces the
    /// tool list, so calling it afterwards would remove the SkillTool.
    pub fn with_skills(mut self, skills: crate::tools::skill::SkillSet) -> Self {
        if skills.is_empty() {
            return self;
        }
        let prompt_fragment = skills.format_for_prompt();
        if self.system_prompt.is_empty() {
            self.system_prompt = prompt_fragment;
        } else {
            self.system_prompt = format!("{}\n\n{}", self.system_prompt, prompt_fragment);
        }
        self.tools
            .push(Box::new(crate::tools::skill::SkillTool::new(
                std::sync::Arc::new(skills),
            )));
        self
    }

    pub fn with_execution_limits(mut self, limits: ExecutionLimits) -> Self {
        self.execution_limits = Some(limits);
        self
    }

    pub fn with_messages(mut self, msgs: Vec<AgentMessage>) -> Self {
        self.messages = msgs;
        self
    }

    pub fn on_before_turn(
        mut self,
        f: impl Fn(&[AgentMessage], usize) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.before_turn = Some(Arc::new(f));
        self
    }

    pub fn on_after_turn(
        mut self,
        f: impl Fn(&[AgentMessage], &Usage) + Send + Sync + 'static,
    ) -> Self {
        self.after_turn = Some(Arc::new(f));
        self
    }

    /// Add an input filter. Filters run in order on user messages before the LLM call.
    pub fn with_input_filter(mut self, filter: impl InputFilter + 'static) -> Self {
        self.input_filters.push(Arc::new(filter));
        self
    }

    /// Set a custom compaction strategy. When set, replaces the default
    /// `compact_messages()` call during context compaction.
    pub fn with_compaction_strategy(mut self, strategy: impl CompactionStrategy + 'static) -> Self {
        self.compaction_strategy = Some(Arc::new(strategy));
        self
    }

    /// Disable automatic context compaction and execution limits.
    /// This takes precedence over auto-derivation from `ModelConfig.context_window`.
    pub fn without_context_management(mut self) -> Self {
        self.context_config = None;
        self.context_management_disabled = true;
        self.execution_limits = None;
        self
    }

    // -- State access --

    pub fn messages(&self) -> &[AgentMessage] {
        &self.messages
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    pub fn set_tools(&mut self, tools: Vec<Box<dyn AgentTool>>) {
        self.tools = tools;
    }

    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    pub fn append_message(&mut self, msg: AgentMessage) {
        self.messages.push(msg);
    }

    pub fn replace_messages(&mut self, msgs: Vec<AgentMessage>) {
        self.messages = msgs;
    }

    pub fn save_messages(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.messages)
    }

    pub fn restore_messages(&mut self, json: &str) -> Result<(), serde_json::Error> {
        let msgs: Vec<AgentMessage> = serde_json::from_str(json)?;
        self.messages = msgs;
        Ok(())
    }

    // -- Queue management --

    /// Queue a steering message (delegates to last run handle).
    pub fn steer(&self, msg: AgentMessage) {
        if let Some(ref h) = self.last_run_handle {
            h.steer(msg);
        } else {
            self.steering_queue.lock().push(msg);
        }
    }

    /// Queue a follow-up message (delegates to last run handle).
    pub fn follow_up(&self, msg: AgentMessage) {
        if let Some(ref h) = self.last_run_handle {
            h.follow_up(msg);
        } else {
            self.follow_up_queue.lock().push(msg);
        }
    }

    pub fn clear_steering_queue(&self) {
        self.steering_queue.lock().clear();
        if let Some(ref h) = self.last_run_handle {
            h.clear_steering();
        }
    }

    pub fn clear_follow_up_queue(&self) {
        self.follow_up_queue.lock().clear();
        if let Some(ref h) = self.last_run_handle {
            h.clear_follow_up();
        }
    }

    pub fn clear_all_queues(&self) {
        self.clear_steering_queue();
        self.clear_follow_up_queue();
    }

    pub fn set_steering_mode(&mut self, mode: QueueMode) {
        self.steering_mode = mode;
    }

    pub fn set_follow_up_mode(&mut self, mode: QueueMode) {
        self.follow_up_mode = mode;
    }

    /// Get the last run handle (if any).
    pub fn run_handle(&self) -> Option<&RunHandle> {
        self.last_run_handle.as_ref()
    }

    // -- Control --

    pub fn abort(&self) {
        if let Some(ref h) = self.last_run_handle {
            h.abort();
        } else if let Some(ref cancel) = self.cancel {
            cancel.cancel();
        }
    }

    pub async fn reset(&mut self) {
        // Cancel cooperatively first, then await to recover tools
        if let Some(ref h) = self.last_run_handle {
            h.abort();
        } else if let Some(ref cancel) = self.cancel {
            cancel.cancel();
        }
        if let Some(handle) = self.pending_completion.take() {
            // Await the cancelled task to recover tools; ignore panic
            if let Ok((tools, _messages)) = handle.await {
                self.tools = tools;
            }
        }
        self.messages.clear();
        self.clear_all_queues();
        self.is_streaming = false;
        self.cancel = None;
        self.last_run_handle = None;
    }

    // -- Submitting --

    pub async fn submit_text(
        &mut self,
        text: impl Into<String>,
    ) -> (RunHandle, mpsc::UnboundedReceiver<AgentEvent>) {
        let msg = AgentMessage::Llm(Message::user(text));
        self.submit(vec![msg]).await
    }

    pub async fn submit(
        &mut self,
        messages: Vec<AgentMessage>,
    ) -> (RunHandle, mpsc::UnboundedReceiver<AgentEvent>) {
        self.finish().await;

        assert!(
            !self.is_streaming,
            "Agent is already streaming. Use steer() or follow_up()."
        );

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.is_streaming = true;

        // Create per-run queues, draining any pre-queued messages
        let run_steering = Arc::new(Mutex::new(self.steering_queue.lock().drain(..).collect()));
        let run_follow_up = Arc::new(Mutex::new(self.follow_up_queue.lock().drain(..).collect()));
        let run_handle = RunHandle {
            steering_queue: run_steering.clone(),
            follow_up_queue: run_follow_up.clone(),
            cancel: cancel.clone(),
        };
        self.last_run_handle = Some(run_handle.clone());

        let (tx, rx) = mpsc::unbounded_channel();

        let mut context = AgentContext {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: std::mem::take(&mut self.tools),
            cwd: self.cwd.clone(),
            path_guard: self.path_guard.clone(),
        };

        let config = self.build_config_with_queues(run_steering, run_follow_up);

        let handle = tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(async {
                let _new_messages =
                    agent_loop(messages, &mut context, &config, tx.clone(), cancel).await;
            });
            if let Err(e) = futures::FutureExt::catch_unwind(result).await {
                let msg = match e.downcast_ref::<&str>() {
                    Some(s) => s.to_string(),
                    None => match e.downcast_ref::<String>() {
                        Some(s) => s.clone(),
                        None => "unknown panic".into(),
                    },
                };
                tx.send(AgentEvent::Error {
                    error: AgentErrorInfo {
                        kind: AgentErrorKind::Runtime,
                        message: format!("Agent loop panicked: {msg}"),
                    },
                })
                .ok();
                tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
            }
            (context.tools, context.messages)
        });

        self.pending_completion = Some(handle);
        (run_handle, rx)
    }

    pub async fn resume(&mut self) -> (RunHandle, mpsc::UnboundedReceiver<AgentEvent>) {
        self.finish().await;

        let (tx, rx) = mpsc::unbounded_channel();

        if self.is_streaming {
            tx.send(AgentEvent::Error {
                error: AgentErrorInfo {
                    kind: AgentErrorKind::Runtime,
                    message: "Agent is already streaming, skipping resume".into(),
                },
            })
            .ok();
            return (RunHandle::noop(), rx);
        }
        if self.messages.is_empty() {
            tx.send(AgentEvent::Error {
                error: AgentErrorInfo {
                    kind: AgentErrorKind::Runtime,
                    message: "No messages to resume from, skipping resume".into(),
                },
            })
            .ok();
            return (RunHandle::noop(), rx);
        }

        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        self.is_streaming = true;

        let run_steering = Arc::new(Mutex::new(self.steering_queue.lock().drain(..).collect()));
        let run_follow_up = Arc::new(Mutex::new(self.follow_up_queue.lock().drain(..).collect()));
        let run_handle = RunHandle {
            steering_queue: run_steering.clone(),
            follow_up_queue: run_follow_up.clone(),
            cancel: cancel.clone(),
        };
        self.last_run_handle = Some(run_handle.clone());

        let mut context = AgentContext {
            system_prompt: self.system_prompt.clone(),
            messages: self.messages.clone(),
            tools: std::mem::take(&mut self.tools),
            cwd: self.cwd.clone(),
            path_guard: self.path_guard.clone(),
        };

        let config = self.build_config_with_queues(run_steering, run_follow_up);

        let handle = tokio::spawn(async move {
            let result = std::panic::AssertUnwindSafe(async {
                let _new_messages =
                    agent_loop_continue(&mut context, &config, tx.clone(), cancel).await;
            });
            if let Err(e) = futures::FutureExt::catch_unwind(result).await {
                let msg = match e.downcast_ref::<&str>() {
                    Some(s) => s.to_string(),
                    None => match e.downcast_ref::<String>() {
                        Some(s) => s.clone(),
                        None => "unknown panic".into(),
                    },
                };
                tx.send(AgentEvent::Error {
                    error: AgentErrorInfo {
                        kind: AgentErrorKind::Runtime,
                        message: format!("Agent loop panicked: {msg}"),
                    },
                })
                .ok();
                tx.send(AgentEvent::AgentEnd { messages: vec![] }).ok();
            }
            (context.tools, context.messages)
        });

        self.pending_completion = Some(handle);
        (run_handle, rx)
    }

    pub async fn finish(&mut self) {
        if let Some(handle) = self.pending_completion.take() {
            match handle.await {
                Ok((tools, messages)) => {
                    self.tools = tools;
                    self.messages = messages;
                }
                Err(e) => {
                    tracing::error!("Agent loop task failed: {}", e);
                }
            }
            self.is_streaming = false;
            self.cancel = None;
            self.last_run_handle = None;
        }
    }

    // -- Internal --

    fn build_config_with_queues(
        &self,
        steering_queue: Arc<Mutex<Vec<AgentMessage>>>,
        follow_up_queue: Arc<Mutex<Vec<AgentMessage>>>,
    ) -> AgentLoopConfig {
        let steering_mode = self.steering_mode;
        let follow_up_mode = self.follow_up_mode;

        AgentLoopConfig {
            provider: self.provider.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            thinking_level: self.thinking_level,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            model_config: self.model_config.clone(),
            convert_to_llm: None,
            transform_context: None,
            get_steering_messages: Some(Box::new(move || {
                let mut queue = steering_queue.lock();
                match steering_mode {
                    QueueMode::OneAtATime => {
                        if queue.is_empty() {
                            vec![]
                        } else {
                            vec![queue.remove(0)]
                        }
                    }
                    QueueMode::All => queue.drain(..).collect(),
                }
            })),
            context_config: if self.context_management_disabled {
                None
            } else {
                Some(self.context_config.clone().unwrap_or_else(|| {
                    self.model_config
                        .as_ref()
                        .map(|m| ContextConfig::from_context_window(m.context_window))
                        .unwrap_or_default()
                }))
            },
            compaction_strategy: self.compaction_strategy.clone(),
            execution_limits: self.execution_limits.clone(),
            cache_config: self.cache_config.clone(),
            tool_execution: self.tool_execution.clone(),
            retry_policy: self.retry_policy.clone(),
            get_follow_up_messages: Some(Box::new(move || {
                let mut queue = follow_up_queue.lock();
                match follow_up_mode {
                    QueueMode::OneAtATime => {
                        if queue.is_empty() {
                            vec![]
                        } else {
                            vec![queue.remove(0)]
                        }
                    }
                    QueueMode::All => queue.drain(..).collect(),
                }
            })),
            before_turn: self.before_turn.clone(),
            after_turn: self.after_turn.clone(),
            input_filters: self.input_filters.clone(),
        }
    }
}
