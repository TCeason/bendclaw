use crate::types::AgentMessage;

#[derive(Clone, Copy)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Clone)]
pub(crate) struct EvictionPlan {
    pub span: Span,
    pub marker: Option<AgentMessage>,
    pub after_tokens: usize,
}
