//! The user-request round: which of the user's requests are still in play.
//! Its answer is the task every call is then judged against.

use tokio_util::sync::CancellationToken;

use super::options::PruneOptions;
use super::report::RequestRelevance;
use super::state::abridge;
use super::state::text_of;
use crate::judge::Answer;
use crate::judge::Judge;
use crate::judge::JudgeError;
use crate::judge::Question;
use crate::types::AgentMessage;
use crate::types::Message;

/// User requests judged per round. Older ones are out of play by age.
const MAX_REQUESTS_JUDGED: usize = 40;
/// Characters of a request shown to the judge.
const REQUEST_CHARS: usize = 300;
/// Characters of the assistant's closing reply shown next to a request.
const CLOSING_CHARS: usize = 200;
/// Characters of a request kept in the task header and the report.
const TASK_REQUEST_CHARS: usize = 300;
/// When the round fails, only this many latest requests count as in play:
/// the task header must not swell to every request ever made.
const IN_PLAY_ON_FAILURE: usize = 5;

/// A user request and how the assistant left it: the last text reply before
/// the next request (or the end of the history).
struct UserRequest {
    message_index: usize,
    text: String,
    closing: Option<String>,
}

fn user_requests(messages: &[AgentMessage]) -> Vec<UserRequest> {
    let mut requests: Vec<UserRequest> = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        match message {
            AgentMessage::Llm(Message::User { content, .. }) => {
                let text = text_of(content);
                if text.trim().is_empty() {
                    continue;
                }
                requests.push(UserRequest {
                    message_index: index,
                    text,
                    closing: None,
                });
            }
            AgentMessage::Llm(Message::Assistant { content, .. }) => {
                let text = text_of(content);
                if text.trim().is_empty() {
                    continue;
                }
                if let Some(last) = requests.last_mut() {
                    last.closing = Some(text);
                }
            }
            _ => {}
        }
    }
    requests
}

/// Ask the judge which user requests are still in play. Returns the verdicts
/// and how many judge requests it took (0 when nothing needed asking). A
/// failed round degrades to "the latest few are in play" rather than
/// blocking the call round.
pub(super) async fn judge_user_requests(
    messages: &[AgentMessage],
    judge: &dyn Judge,
    options: &PruneOptions,
    cancel: CancellationToken,
) -> Result<(Vec<RequestRelevance>, usize), JudgeError> {
    let all = user_requests(messages);
    let skip = all.len().saturating_sub(MAX_REQUESTS_JUDGED);
    let mut report: Vec<RequestRelevance> = all
        .iter()
        .map(|r| RequestRelevance {
            message_index: r.message_index,
            text: abridge(&r.text, TASK_REQUEST_CHARS),
            probability: None,
            in_play: false,
        })
        .collect();
    if let Some(latest) = report.last_mut() {
        latest.in_play = true;
    }
    let asked: Vec<&UserRequest> = all.iter().skip(skip).collect();
    // The latest request is in play by definition; nothing to ask about
    // when it is the only one.
    if asked.len() < 2 {
        return Ok((report, 0));
    }
    let state = requests_state(&asked, messages.len());
    let questions: Vec<Question> = asked
        .iter()
        .take(asked.len() - 1)
        .map(|r| {
            Question::noul(
                format!("request_{}", r.message_index),
                format!("Is the user's request at message {} still part of what the user is working on now?", r.message_index),
            )
            .with_criteria(
                "the current work continues it, builds on it, or refers back to it",
                "finished and closed, abandoned, or a greeting or aside with no task in it",
            )
        })
        .collect();
    match judge.ask(&state, &questions, cancel).await {
        Ok(answers) => {
            for entry in report.iter_mut().skip(skip) {
                if entry.probability.is_some() || entry.in_play {
                    continue;
                }
                let probability = answers
                    .get(&format!("request_{}", entry.message_index))
                    .and_then(Answer::probability);
                entry.probability = probability;
                // Unanswered: keep it in play rather than pretend it closed.
                entry.in_play = probability.is_none_or(|p| p >= options.keep_threshold);
            }
        }
        Err(JudgeError::Cancelled) => return Err(JudgeError::Cancelled),
        Err(error) => {
            tracing::warn!(error = %error, "judge prune: user request round failed; latest requests count as in play");
            let from = report.len().saturating_sub(IN_PLAY_ON_FAILURE);
            for entry in report.iter_mut().skip(from) {
                entry.in_play = true;
            }
        }
    }
    Ok((report, 1))
}

/// The state of the user-request round: every request with the assistant's
/// closing reply, oldest first, message indices in brackets.
fn requests_state(requests: &[&UserRequest], total_messages: usize) -> String {
    let mut lines = vec![format!(
        "# User requests, oldest first (message index in brackets; {total_messages} messages so far)\nEach request is followed by how the assistant left it. The last request is the one being worked on now."
    )];
    for (n, request) in requests.iter().enumerate() {
        let latest = n + 1 == requests.len();
        lines.push(format!(
            "[{}] user: {}{}",
            request.message_index,
            abridge(&request.text, REQUEST_CHARS),
            if latest { "   ← now" } else { "" }
        ));
        if let Some(closing) = &request.closing {
            lines.push(format!(
                "    assistant: {}",
                abridge(closing, CLOSING_CHARS)
            ));
        }
    }
    lines.join("\n")
}

/// The task, once per batch: the user requests still in play, with their
/// message indices so the judge can tell which one a stretch of
/// conversation served. What "still matters" is judged against these.
pub(super) fn task_header(requests: &[RequestRelevance]) -> String {
    let mut lines =
        vec!["# Task (user requests still in play, message index in brackets)".to_string()];
    for request in requests.iter().filter(|r| r.in_play) {
        lines.push(format!("[{}] {}", request.message_index, request.text));
    }
    lines.join("\n")
}
