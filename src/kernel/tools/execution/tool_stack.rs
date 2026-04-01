//! Factory that assembles the full tool execution stack.
//!
//! Hides CallExecutor / ExecutionRecorder / EventEmitter wiring
//! so callers only deal with ToolStack.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::events::EventEmitter;
use super::executor::CallExecutor;
use super::labels::ExecutionLabels;
use super::lifecycle::ToolLifecycle;
use super::recorder::ExecutionRecorder;
use crate::kernel::run::event::Event;
use crate::kernel::skills::executor::SkillExecutor;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::ToolContext;
use crate::kernel::trace::Trace;

pub struct ToolStackConfig {
    pub tool_registry: Arc<ToolRegistry>,
    pub skill_executor: Arc<dyn SkillExecutor>,
    pub tool_context: ToolContext,
    pub labels: Arc<ExecutionLabels>,
    pub cancel: CancellationToken,
    pub trace: Trace,
    pub event_tx: mpsc::Sender<Event>,
    pub allowed_tool_names: Option<HashSet<String>>,
}

pub struct ToolStack {
    pub lifecycle: ToolLifecycle,
}

impl ToolStack {
    pub fn build(config: ToolStackConfig) -> Self {
        let executor = CallExecutor::new(
            config.tool_registry,
            config.skill_executor,
            config.tool_context,
            config.cancel,
            config.event_tx.clone(),
        )
        .with_allowed_tool_names(config.allowed_tool_names);
        let recorder = ExecutionRecorder::new(config.labels, config.trace, config.event_tx.clone());
        let emitter = EventEmitter::new(config.event_tx);
        Self {
            lifecycle: ToolLifecycle::new(executor, recorder, emitter),
        }
    }
}
