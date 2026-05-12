use super::phase::PhaseContext;
use super::phase::PhaseResult;
use super::pressure::EvictMode;
use super::pressure::PressureState;
use super::snapshot::ContextSnapshot;
use super::transforms::level0_reclaim::current_run;
use super::transforms::level0_reclaim::image_path_downgrade;
use super::transforms::level1_shrink;
use super::transforms::level2_collapse::old_turns;
use super::transforms::level3_evict::stale;
use crate::types::AgentMessage;

pub struct LevelInput<'a> {
    pub snapshot: &'a ContextSnapshot,
    pub pressure: &'a PressureState,
    pub phase_context: &'a PhaseContext,
    pub current_tokens: usize,
}

pub trait CompactionLevel {
    fn level(&self) -> u8;

    fn should_run(&self, input: &LevelInput<'_>) -> bool;

    fn run(&self, messages: Vec<AgentMessage>, input: &LevelInput<'_>) -> PhaseResult;
}

pub struct ReclaimLevel;
pub struct ShrinkLevel;
pub struct CollapseLevel;
pub struct EvictLevel;

impl CompactionLevel for ReclaimLevel {
    fn level(&self) -> u8 {
        0
    }

    fn should_run(&self, _input: &LevelInput<'_>) -> bool {
        true
    }

    fn run(&self, messages: Vec<AgentMessage>, input: &LevelInput<'_>) -> PhaseResult {
        let first = current_run::run(messages, input.phase_context);
        let second = image_path_downgrade::run(first.messages, input.phase_context);
        let mut actions = first.actions;
        actions.extend(second.actions);
        PhaseResult {
            messages: second.messages,
            actions,
        }
    }
}

impl CompactionLevel for ShrinkLevel {
    fn level(&self) -> u8 {
        1
    }

    fn should_run(&self, _input: &LevelInput<'_>) -> bool {
        true
    }

    fn run(&self, messages: Vec<AgentMessage>, input: &LevelInput<'_>) -> PhaseResult {
        level1_shrink::run(messages, input.phase_context, input.current_tokens)
    }
}

impl CompactionLevel for CollapseLevel {
    fn level(&self) -> u8 {
        2
    }

    fn should_run(&self, input: &LevelInput<'_>) -> bool {
        input.pressure.needs_collapse(input.snapshot)
    }

    fn run(&self, messages: Vec<AgentMessage>, input: &LevelInput<'_>) -> PhaseResult {
        old_turns::run(messages, input.phase_context, input.current_tokens)
    }
}

impl CompactionLevel for EvictLevel {
    fn level(&self) -> u8 {
        3
    }

    fn should_run(&self, input: &LevelInput<'_>) -> bool {
        input.pressure.needs_evict()
    }

    fn run(&self, messages: Vec<AgentMessage>, input: &LevelInput<'_>) -> PhaseResult {
        match input.pressure.evict_mode() {
            Some(EvictMode::MessageLimit) => {
                stale::drop_to_message_target(messages, input.phase_context)
            }
            Some(EvictMode::TokenBudget) => {
                stale::drop_to_token_target(messages, input.phase_context)
            }
            None => PhaseResult {
                messages,
                actions: Vec::new(),
            },
        }
    }
}

pub fn default_levels() -> Vec<Box<dyn CompactionLevel>> {
    vec![
        Box::new(ReclaimLevel),
        Box::new(ShrinkLevel),
        Box::new(CollapseLevel),
        Box::new(EvictLevel),
    ]
}
