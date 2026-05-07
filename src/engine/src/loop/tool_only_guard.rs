//! Tool-only progress guard: intervene when the agent keeps using tools
//! without producing any visible answer or conclusion.

use crate::types::*;

/// Returned when tool-only turns exceed a threshold.
pub struct ToolOnlyIntervention {
    /// A steering message to inject before the next LLM turn.
    pub steering_message: AgentMessage,
}

/// Tracks consecutive assistant turns that only contain tool calls.
pub struct ToolOnlyGuard {
    threshold: usize,
    consecutive_tool_only_turns: usize,
    intervened: bool,
}

impl ToolOnlyGuard {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            consecutive_tool_only_turns: 0,
            intervened: false,
        }
    }

    /// Record one assistant turn.
    ///
    /// Returns an intervention once when the assistant has emitted tool calls
    /// for `threshold` consecutive turns without any visible text.
    pub fn check(
        &mut self,
        message: &Message,
        has_tool_calls: bool,
    ) -> Option<ToolOnlyIntervention> {
        if !has_tool_calls {
            self.consecutive_tool_only_turns = 0;
            self.intervened = false;
            return None;
        }

        if has_visible_text(message) {
            self.consecutive_tool_only_turns = 0;
            self.intervened = false;
            return None;
        }

        self.consecutive_tool_only_turns += 1;
        if self.consecutive_tool_only_turns >= self.threshold && !self.intervened {
            self.intervened = true;
            tracing::warn!(
                count = self.consecutive_tool_only_turns,
                "tool-only turn streak detected — asking agent to summarize"
            );
            Some(Self::build_intervention(self.consecutive_tool_only_turns))
        } else {
            None
        }
    }

    fn build_intervention(count: usize) -> ToolOnlyIntervention {
        // Wording is deliberately flat and declarative. Earlier drafts used
        // imperatives like "Continue" and templates like
        // "briefly state the user's goal, then continue" — the model would
        // mimic the template in the next turn, producing `<system>继续：…`
        // or `Continue: …` preambles copied straight from the reminder.
        // Keep it as status text without verbs the model can ape.
        let warning = format!(
            "<system-reminder>\n\
             Status: {count} consecutive tool calls without any assistant text. \
             Not a limit, just a note. The next assistant text block should \
             include a short progress update, written naturally as part of \
             your reply — do not prefix it with 'Continue', 'Next step:', or \
             any status template.\n\
             </system-reminder>"
        );

        ToolOnlyIntervention {
            steering_message: AgentMessage::Llm(Message::User {
                content: vec![Content::Text { text: warning }],
                timestamp: now_ms(),
            }),
        }
    }
}

fn has_visible_text(message: &Message) -> bool {
    match message {
        Message::Assistant { content, .. } => content.iter().any(|c| match c {
            Content::Text { text } => !text.trim().is_empty(),
            _ => false,
        }),
        _ => false,
    }
}
