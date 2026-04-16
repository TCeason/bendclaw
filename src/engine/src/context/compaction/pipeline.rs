//! Compaction pipeline — builder + sequential executor.

use super::compact::CompactionAction;
use super::pass::CompactionContext;
use super::pass::CompactionPass;
use crate::types::AgentMessage;

/// Result of running the full pipeline.
pub struct PipelineResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<CompactionAction>,
}

/// A pipeline of compaction passes executed in sequence.
pub struct CompactionPipeline {
    passes: Vec<Box<dyn CompactionPass>>,
}

impl CompactionPipeline {
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder { passes: vec![] }
    }

    pub fn execute(self, messages: Vec<AgentMessage>, ctx: &CompactionContext) -> PipelineResult {
        let mut current = messages;
        let mut all_actions: Vec<CompactionAction> = vec![];
        for pass in &self.passes {
            let result = pass.run(current, ctx);
            all_actions.extend(result.actions);
            current = result.messages;
        }
        PipelineResult {
            messages: current,
            actions: all_actions,
        }
    }
}

pub struct PipelineBuilder {
    passes: Vec<Box<dyn CompactionPass>>,
}

impl PipelineBuilder {
    pub fn add(mut self, pass: impl CompactionPass + 'static) -> Self {
        self.passes.push(Box::new(pass));
        self
    }

    pub fn build(self) -> CompactionPipeline {
        CompactionPipeline {
            passes: self.passes,
        }
    }
}
