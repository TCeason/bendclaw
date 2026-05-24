use super::config::CompactionConfig;
use super::pressure::Pressure;
use super::types::CompactionAction;
use crate::types::AgentMessage;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PassLevel {
    Reclaim = 0,
    Shrink = 1,
    Microcompact = 2,
    Collapse = 3,
    Evict = 4,
}

impl PassLevel {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

pub struct PassContext<'a> {
    pub config: &'a CompactionConfig,
    pub pressure: Pressure,
}

pub struct PassResult {
    pub messages: Vec<AgentMessage>,
    pub actions: Vec<CompactionAction>,
}

pub trait Pass: Send + Sync {
    fn level(&self) -> PassLevel;
    fn should_run(&self, ctx: &PassContext<'_>) -> bool;
    fn run(&self, messages: Vec<AgentMessage>, ctx: &PassContext<'_>) -> PassResult;
}
