//! The two questions asked about each candidate, and the verdict their
//! answers add up to.

use std::collections::HashMap;

use super::candidates::Candidate;
use super::options::PruneOptions;
use super::report::Decision;
use super::report::Verdict;
use super::state::abridge;
use crate::judge::Answer;
use crate::judge::Judge;
use crate::judge::Question;
use crate::judge::QuestionKind;

/// Questions per candidate: keep the call, keep the result.
pub(super) const PER_CANDIDATE: usize = 2;

/// Characters of the call arguments quoted in a question. The question only
/// has to identify the call; the state carries the arguments (fitted to the
/// budget). Unbounded, a `Write` of a large file made one question larger than
/// the judge's window and the whole batch failed.
const QUESTION_ARGUMENT_CHARS: usize = 240;

/// Tokens the schema around one question costs on the wire, beyond its text.
const SCHEMA_OVERHEAD_TOKENS: usize = 12;

pub(super) fn for_candidate(candidate: &Candidate) -> Vec<Question> {
    let (call_id, result_id) = ids(candidate);
    let about = format!(
        "the {} call `{}` (arguments {}; result {})",
        candidate.tool_name,
        candidate.call_id,
        abridge(&candidate.arguments, QUESTION_ARGUMENT_CHARS),
        candidate.result_note
    );
    vec![
        Question::noul(
            call_id,
            format!("Does knowing that {about} was made still matter for the ongoing task?"),
        )
        .with_criteria(
            "the task still depends on this having happened",
            "irrelevant now; forgetting it changes nothing",
        ),
        Question::noul(
            result_id,
            format!("Is the verbatim content of the result of {about} still needed?"),
        )
        .with_criteria(
            "its contents will be read again and re-running the tool is not an option",
            "superseded, already acted on, or cheap to re-run",
        ),
    ]
}

/// Estimated tokens the questions add to a request: instructions, criteria
/// and ids, plus the schema around each.
pub(super) fn tokens(questions: &[Question], judge: &dyn Judge) -> usize {
    questions
        .iter()
        .map(|q| {
            let criteria = match &q.kind {
                QuestionKind::Noul { yes, no } => {
                    yes.as_deref().map_or(0, |t| judge.estimate_tokens(t))
                        + no.as_deref().map_or(0, |t| judge.estimate_tokens(t))
                }
                _ => 0,
            };
            judge.estimate_tokens(&q.instructions)
                + judge.estimate_tokens(&q.id)
                + criteria
                + SCHEMA_OVERHEAD_TOKENS
        })
        .sum()
}

fn ids(candidate: &Candidate) -> (String, String) {
    let key: String = candidate
        .call_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    (format!("call_{key}"), format!("result_{key}"))
}

/// `keep_result >= threshold` keeps both; else `keep_call >= threshold` keeps
/// the call and truncates the result; else the pair goes. Silence never
/// deletes: an unanswered question counts as a keep for what it asked about.
pub(super) fn verdict(
    candidate: &Candidate,
    answers: &HashMap<String, Answer>,
    options: &PruneOptions,
) -> Verdict {
    let (call_id, result_id) = ids(candidate);
    let keep_call = answers.get(&call_id).and_then(Answer::probability);
    let keep_result = answers.get(&result_id).and_then(Answer::probability);
    let threshold = options.keep_threshold;
    let decision = match (keep_call, keep_result) {
        (_, Some(result)) if result >= threshold => Decision::Keep,
        (Some(call), _) if call >= threshold => Decision::Truncate,
        (None, None) => Decision::Keep,
        (None, Some(_)) => Decision::Truncate,
        _ => Decision::Remove,
    };
    let saves_tokens = match decision {
        Decision::Keep => 0,
        Decision::Truncate => candidate.truncation_tokens,
        Decision::Remove => candidate.pair_tokens,
    };
    Verdict {
        call_id: candidate.call_id.clone(),
        tool_name: candidate.tool_name.clone(),
        arguments: candidate.arguments.chars().take(200).collect(),
        decision,
        keep_call,
        keep_result,
        saves_tokens,
    }
}
