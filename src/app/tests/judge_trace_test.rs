//! `judge-trace.jsonl`: every judge request of a session, with its state,
//! questions and answers, appended as one JSON line each.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use evot::judge::TracingJudge;
use evot::judge::JUDGE_TRACE_FILE;
use evot::judge::JUDGE_TRACE_VERSION;
use evot_engine::judge::Answer;
use evot_engine::judge::Judge;
use evot_engine::judge::JudgeError;
use evot_engine::judge::Question;
use tokio_util::sync::CancellationToken;

struct Scripted {
    fail: bool,
}

#[async_trait]
impl Judge for Scripted {
    async fn ask(
        &self,
        _state: &str,
        questions: &[Question],
        _cancel: CancellationToken,
    ) -> Result<HashMap<String, Answer>, JudgeError> {
        if self.fail {
            return Err(JudgeError::Transport("gateway down".into()));
        }
        Ok(questions
            .iter()
            .map(|q| (q.id.clone(), Answer::Noul { probability: 0.25 }))
            .collect())
    }
}

fn read_lines(dir: &std::path::Path) -> Vec<serde_json::Value> {
    let raw = std::fs::read_to_string(dir.join(JUDGE_TRACE_FILE)).unwrap_or_default();
    raw.lines()
        .map(|line| serde_json::from_str(line).unwrap_or(serde_json::Value::Null))
        .collect()
}

#[tokio::test]
async fn records_state_questions_and_answers_as_jsonl() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir: {error}"),
    };
    let judge = TracingJudge::new(Arc::new(Scripted { fail: false }), dir.path().to_path_buf());
    let questions =
        vec![Question::noul("keep_a", "Still needed?").with_criteria("needed", "stale")];

    let answers = judge
        .ask("# Task\nfix it", &questions, CancellationToken::new())
        .await;
    assert!(answers.is_ok(), "the trace must hand the answer through");
    let second = judge
        .ask("# Task\nanother", &questions, CancellationToken::new())
        .await;
    assert!(second.is_ok());

    let lines = read_lines(dir.path());
    assert_eq!(lines.len(), 2, "one line per request, appended");
    let first = &lines[0];
    assert_eq!(first["version"], JUDGE_TRACE_VERSION);
    assert_eq!(first["state"], "# Task\nfix it");
    assert_eq!(first["questions"][0]["id"], "keep_a");
    assert_eq!(first["questions"][0]["kind"], "noul");
    assert_eq!(first["questions"][0]["yes"], "needed");
    assert_eq!(first["questions"][0]["no"], "stale");
    assert_eq!(first["answers"]["keep_a"]["probability"], 0.25);
    assert!(first.get("error").is_none());
    assert!(first["ts_ms"].as_u64().is_some());
    assert!(first["elapsed_ms"].as_u64().is_some());
    assert_eq!(lines[1]["state"], "# Task\nanother");
}

#[tokio::test]
async fn records_failures_and_still_returns_them() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir: {error}"),
    };
    let judge = TracingJudge::new(Arc::new(Scripted { fail: true }), dir.path().to_path_buf());
    let questions = vec![Question::noul("q", "?")];
    let result = judge
        .ask("state", &questions, CancellationToken::new())
        .await;
    assert!(matches!(result, Err(JudgeError::Transport(_))));

    let lines = read_lines(dir.path());
    assert_eq!(lines.len(), 1);
    assert!(lines[0].get("answers").is_none());
    assert_eq!(lines[0]["error"], "judge request failed: gateway down");
}

#[tokio::test]
async fn creates_the_session_directory_when_missing() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir: {error}"),
    };
    let session_dir = dir.path().join("sessions").join("s1");
    let judge = TracingJudge::new(Arc::new(Scripted { fail: false }), session_dir.clone());
    let result = judge
        .ask(
            "state",
            &[Question::noul("q", "?")],
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_ok());
    assert_eq!(read_lines(&session_dir).len(), 1);
}
