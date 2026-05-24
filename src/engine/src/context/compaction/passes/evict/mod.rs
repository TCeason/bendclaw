mod apply;
mod bounds;
mod pass;
mod planner;
mod units;

pub use pass::Evict;

use crate::types::AgentMessage;

#[derive(Clone, Copy)]
pub(super) struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Clone)]
pub(super) struct EvictionPlan {
    pub span: Span,
    pub marker: Option<AgentMessage>,
    pub after_tokens: usize,
}
