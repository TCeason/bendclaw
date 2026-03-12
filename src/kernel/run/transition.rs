use std::time::Duration;

use crate::base::ErrorSource;
use crate::kernel::run::orchestration::aborted_tool_result_messages;
use crate::kernel::run::orchestration::record_assistant_turn;
use crate::kernel::run::result::Reason;
use crate::kernel::run::run_loop::LLMResponse;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::Message;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnTransition {
    Error(Reason),
    Abort(Reason),
    DispatchTools,
    Done,
}

pub fn apply_turn_result(
    messages: &mut Vec<Message>,
    state: &mut RunLoopState,
    turn: &LLMResponse,
    llm_error: Option<&str>,
    abort_reason: Option<Reason>,
    model: &str,
    max_duration: Duration,
) -> TurnTransition {
    if let Some(err) = llm_error {
        messages.push(Message::operation_event(
            "llm",
            "reasoning.turn",
            "failed",
            serde_json::json!({"finish_reason": turn.finish_reason(), "error": err}),
        ));
        messages.push(Message::error(ErrorSource::Llm, err));
        state.record_error(err);
        return TurnTransition::Error(Reason::Error);
    }

    record_assistant_turn(messages, turn, state, model, max_duration);

    if turn.has_tool_calls() && state.should_continue() {
        if let Some(reason) = abort_reason {
            messages.extend(aborted_tool_result_messages(turn.tool_calls()));
            return TurnTransition::Abort(reason);
        }
        return TurnTransition::DispatchTools;
    }

    TurnTransition::Done
}
