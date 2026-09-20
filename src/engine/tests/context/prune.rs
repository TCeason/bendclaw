//! Prune decide: the user-request round shapes the task header the call
//! round is judged against, and the report records both.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use evotengine::context::compaction::prune::DecideReport;
use evotengine::context::compaction::PruneLedger;
use evotengine::context::compaction::PruneOptions;
use evotengine::judge::Answer;
use evotengine::judge::Judge;
use evotengine::judge::JudgeError;
use evotengine::judge::Question;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;

const REQUEST_ROUND_HEAD: &str = "# User requests";

/// Answers the request round from `request_probs` (by message index) and
/// keeps every call in the call round; records every state it read.
#[derive(Default)]
struct FakeJudge {
    request_probs: HashMap<usize, f64>,
    fail_request_round: bool,
    states: Mutex<Vec<(String, Vec<Question>)>>,
}

impl FakeJudge {
    fn states(&self) -> Vec<(String, Vec<Question>)> {
        self.states.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn request_states(&self) -> Vec<String> {
        self.states()
            .into_iter()
            .filter(|(s, _)| s.starts_with(REQUEST_ROUND_HEAD))
            .map(|(s, _)| s)
            .collect()
    }

    fn call_states(&self) -> Vec<String> {
        self.states()
            .into_iter()
            .filter(|(s, _)| !s.starts_with(REQUEST_ROUND_HEAD))
            .map(|(s, _)| s)
            .collect()
    }
}

#[async_trait]
impl Judge for FakeJudge {
    async fn ask(
        &self,
        state: &str,
        questions: &[Question],
        _cancel: CancellationToken,
    ) -> Result<HashMap<String, Answer>, JudgeError> {
        if let Ok(mut states) = self.states.lock() {
            states.push((state.to_string(), questions.to_vec()));
        }
        if state.starts_with(REQUEST_ROUND_HEAD) {
            if self.fail_request_round {
                return Err(JudgeError::Transport("boom".into()));
            }
            return Ok(questions
                .iter()
                .filter_map(|q| {
                    let index: usize = q.id.strip_prefix("request_")?.parse().ok()?;
                    let probability = *self.request_probs.get(&index)?;
                    Some((q.id.clone(), Answer::Noul { probability }))
                })
                .collect());
        }
        Ok(questions
            .iter()
            .map(|q| (q.id.clone(), Answer::Noul { probability: 0.9 }))
            .collect())
    }
}

fn user(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::user(text))
}

fn assistant(text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::Text { text: text.into() }],
        stop_reason: StopReason::Stop,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn call(id: &str) -> AgentMessage {
    AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: id.into(),
            name: "read".into(),
            arguments: serde_json::json!({ "path": format!("{id}.rs") }),
            metadata: None,
        }],
        stop_reason: StopReason::ToolUse,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    })
}

fn result(id: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.into(),
        tool_name: "read".into(),
        content: vec![Content::Text {
            text: "fn main() {}".repeat(20),
        }],
        is_error: false,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

/// hi → fix spacing (done) → analyse persistence (ongoing), with tool calls
/// under each task and a long enough tail that they are not pinned.
fn history() -> Vec<AgentMessage> {
    vec![
        user("hi"),                                                   // 0
        assistant("Hello! What can I do for you?"),                   // 1
        user("fix the jev prune card spacing"),                       // 2
        call("c1"),                                                   // 3
        result("c1"),                                                 // 4
        assistant("Fixed the spacing; 69 tests pass."),               // 5
        user("analyse whether jev scores persist to the transcript"), // 6
        call("c2"),                                                   // 7
        result("c2"),                                                 // 8
        assistant("Looking at the projection…"),                      // 9
        user("and what triggers a prune?"),                           // 10
        assistant("Let me check."),                                   // 11
        assistant("a"),                                               // 12
        assistant("b"),                                               // 13
        assistant("c"),                                               // 14
        assistant("d"),                                               // 15
        assistant("e"),                                               // 16
    ]
}

async fn decide(judge: &FakeJudge, messages: &[AgentMessage]) -> DecideReport {
    let mut ledger = PruneLedger::default();
    match ledger
        .decide(
            messages,
            judge,
            &PruneOptions::default(),
            CancellationToken::new(),
        )
        .await
    {
        Ok(report) => report,
        Err(error) => panic!("decide failed: {error}"),
    }
}

#[tokio::test]
async fn request_round_shapes_the_task_header() {
    let judge = FakeJudge {
        request_probs: HashMap::from([(0, 0.05), (2, 0.2), (6, 0.9)]),
        ..Default::default()
    };
    let report = decide(&judge, &history()).await;

    let in_play: Vec<usize> = report
        .user_requests
        .iter()
        .filter(|r| r.in_play)
        .map(|r| r.message_index)
        .collect();
    assert_eq!(
        in_play,
        vec![6, 10],
        "closed and greeting requests drop out; latest is in by definition"
    );
    let latest = report.user_requests.iter().find(|r| r.message_index == 10);
    assert_eq!(
        latest.and_then(|r| r.probability),
        None,
        "latest request is not asked about"
    );

    let call_states = judge.call_states();
    assert_eq!(call_states.len(), 1);
    let state = &call_states[0];
    assert!(
        state.contains("[6] analyse whether jev scores persist"),
        "state:\n{state}"
    );
    assert!(
        state.contains("[10] and what triggers a prune?"),
        "state:\n{state}"
    );
    assert!(
        !state.contains("[0] hi"),
        "greeting must not anchor the task:\n{state}"
    );
    assert!(
        !state.contains("[2] fix the jev prune card spacing"),
        "closed task must not anchor:\n{state}"
    );
    assert_eq!(report.requests, 2, "one request round + one call round");
    assert_eq!(report.batches.len(), 1);
    assert_eq!(report.batches[0].call_ids, vec!["c1", "c2"]);
    assert_eq!((report.batches[0].from, report.batches[0].to), (3, 8));
}

#[tokio::test]
async fn request_round_shows_closing_replies() {
    let judge = FakeJudge::default();
    decide(&judge, &history()).await;
    let states = judge.request_states();
    assert_eq!(states.len(), 1);
    let state = &states[0];
    assert!(
        state.contains("[2] user: fix the jev prune card spacing"),
        "{state}"
    );
    assert!(
        state.contains("assistant: Fixed the spacing; 69 tests pass."),
        "{state}"
    );
    assert!(
        state.contains("[10] user: and what triggers a prune?   ← now"),
        "{state}"
    );
    let questions: Vec<String> = judge.states()[0].1.iter().map(|q| q.id.clone()).collect();
    assert_eq!(
        questions,
        vec!["request_0", "request_2", "request_6"],
        "latest is not asked"
    );
}

#[tokio::test]
async fn unanswered_requests_stay_in_play() {
    // The judge answers only for message 0; 2 and 6 come back silent.
    let judge = FakeJudge {
        request_probs: HashMap::from([(0, 0.05)]),
        ..Default::default()
    };
    let report = decide(&judge, &history()).await;
    let by_index: HashMap<usize, bool> = report
        .user_requests
        .iter()
        .map(|r| (r.message_index, r.in_play))
        .collect();
    assert_eq!(by_index.get(&0), Some(&false));
    assert_eq!(
        by_index.get(&2),
        Some(&true),
        "silence never closes a request"
    );
    assert_eq!(by_index.get(&6), Some(&true));
}

#[tokio::test]
async fn failed_request_round_keeps_every_request_and_still_judges_calls() {
    let judge = FakeJudge {
        fail_request_round: true,
        ..Default::default()
    };
    let report = decide(&judge, &history()).await;
    assert!(report.user_requests.iter().all(|r| r.in_play));
    assert!(report.user_requests.iter().all(|r| r.probability.is_none()));
    assert_eq!(report.verdicts.len(), 2, "the call round still ran");
    let state = &judge.call_states()[0];
    assert!(
        state.contains("[0] hi"),
        "fallback anchors on every request:\n{state}"
    );
}

#[tokio::test]
async fn single_request_skips_the_round() {
    let mut messages = vec![user("read these files"), call("c1"), result("c1")];
    for _ in 0..7 {
        messages.push(assistant("…"));
    }
    let judge = FakeJudge::default();
    let report = decide(&judge, &messages).await;
    assert!(judge.request_states().is_empty());
    assert_eq!(report.requests, 1);
    assert_eq!(report.user_requests.len(), 1);
    assert!(report.user_requests[0].in_play);
    assert!(judge.call_states()[0].contains("[0] read these files"));
}

#[test]
fn decide_report_reads_legacy_json_without_request_fields() {
    let legacy = serde_json::json!({
        "verdicts": [],
        "context_tokens": 100,
        "pending_tokens": 0,
        "requests": 1,
        "elapsed_ms": 5
    });
    let report: Result<DecideReport, _> = serde_json::from_value(legacy);
    match report {
        Ok(report) => {
            assert!(report.user_requests.is_empty());
            assert!(report.batches.is_empty());
        }
        Err(error) => panic!("legacy report must still parse: {error}"),
    }
}

#[test]
fn arc_judge_is_accepted() {
    // Compile-time: the ledger takes `&dyn Judge`, so an `Arc<dyn Judge>`
    // from the app layer derefs in.
    let judge: Arc<dyn Judge> = Arc::new(FakeJudge::default());
    let _: &dyn Judge = judge.as_ref();
}

/// A `Write` of a large file must not become a question larger than Jev's
/// window: the question quotes an abridged argument, the state carries the
/// call. (62 `max_tokens_exceeded` a day in prod before this.)
#[tokio::test]
async fn call_questions_abridge_large_arguments() {
    let big = "x".repeat(200_000);
    let mut messages = history();
    messages[7] = AgentMessage::Llm(Message::Assistant {
        content: vec![Content::ToolCall {
            id: "c2".into(),
            name: "write".into(),
            arguments: serde_json::json!({ "path": "big.txt", "content": big }),
            metadata: None,
        }],
        stop_reason: StopReason::ToolUse,
        model: "test".into(),
        provider: "test".into(),
        usage: Usage::default(),
        timestamp: 0,
        error_message: None,
        response_id: None,
    });
    let judge = FakeJudge::default();
    decide(&judge, &messages).await;
    let call_round: Vec<(String, Vec<Question>)> = judge
        .states()
        .into_iter()
        .filter(|(s, _)| !s.starts_with(REQUEST_ROUND_HEAD))
        .collect();
    assert!(!call_round.is_empty(), "call round asked");
    for (state, questions) in call_round {
        assert!(
            state.chars().count() < 100_000,
            "state fitted to the budget, got {} chars",
            state.chars().count()
        );
        for q in questions {
            assert!(
                q.instructions.chars().count() < 1_000,
                "question {} is {} chars",
                q.id,
                q.instructions.chars().count()
            );
        }
    }
}
