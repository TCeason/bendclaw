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
use evotengine::judge::JudgeLimits;
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
    /// Fail any call-round batch asking about this call id.
    fail_call_batch: Option<String>,
    /// Answer call-round questions from here (by id); default 0.9.
    call_probs: HashMap<String, f64>,
    /// What this judge says it can take; `None` for the default (Jev).
    limits: Option<JudgeLimits>,
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
        if let Some(id) = &self.fail_call_batch {
            if questions.iter().any(|q| q.id == format!("call_{id}")) {
                return Err(JudgeError::Transport("input exceeds model window".into()));
            }
        }
        Ok(questions
            .iter()
            .map(|q| {
                let probability = *self.call_probs.get(&q.id).unwrap_or(&0.9);
                (q.id.clone(), Answer::Noul { probability })
            })
            .collect())
    }

    fn limits(&self) -> JudgeLimits {
        self.limits.unwrap_or_default()
    }
}

/// A judge that takes two questions per request: one candidate per batch.
fn one_call_per_batch() -> JudgeLimits {
    JudgeLimits {
        request_tokens: 22_000,
        max_questions: 2,
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

/// One batch failing (a 400 from Jev) must not void the others: their
/// verdicts land, and the failed calls stay undecided for the next round.
#[tokio::test]
async fn failed_call_batch_keeps_other_batches_and_leaves_its_calls_undecided() {
    let options = PruneOptions::default();
    let judge = FakeJudge {
        fail_call_batch: Some("c1".into()),
        limits: Some(one_call_per_batch()),
        ..Default::default()
    };
    let mut ledger = PruneLedger::default();
    let report = match ledger
        .decide(&history(), &judge, &options, CancellationToken::new())
        .await
    {
        Ok(report) => report,
        Err(error) => panic!("one failed batch must not fail decide: {error}"),
    };
    let judged: Vec<&str> = report.verdicts.iter().map(|v| v.call_id.as_str()).collect();
    assert_eq!(judged, vec!["c2"], "only the surviving batch is recorded");

    assert!(
        report.batches.iter().any(|b| b.failed.is_some()),
        "the failed batch is on record"
    );

    // A later round with a working judge asks about c1 again.
    let judge = FakeJudge {
        limits: Some(one_call_per_batch()),
        ..Default::default()
    };
    let report = match ledger
        .decide(&history(), &judge, &options, CancellationToken::new())
        .await
    {
        Ok(report) => report,
        Err(error) => panic!("decide failed: {error}"),
    };
    assert!(
        report.verdicts.iter().any(|v| v.call_id == "c1"),
        "c1 stayed undecided and is asked again"
    );
}

/// Every batch failing is still a failed round.
#[tokio::test]
async fn all_call_batches_failing_fails_decide() {
    let options = PruneOptions::default();
    let judge = FakeJudge {
        fail_call_batch: Some("c1".into()),
        limits: Some(one_call_per_batch()),
        ..Default::default()
    };
    let mut messages = vec![
        user("read these"),
        user("and this"),
        call("c1"),
        result("c1"),
    ];
    messages.extend((0..6).map(|_| assistant("…")));
    let mut ledger = PruneLedger::default();
    let outcome = ledger
        .decide(&messages, &judge, &options, CancellationToken::new())
        .await;
    assert!(outcome.is_err(), "no batch succeeded");
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

// ------------------------------------------------------------ ledger cycle

use evotengine::context::compaction::prune::Decision;
use evotengine::context::compaction::ApplyTrigger;
use evotengine::context::tokens::total_tokens;

fn result_text(id: &str, text: &str) -> AgentMessage {
    AgentMessage::Llm(Message::ToolResult {
        tool_call_id: id.into(),
        tool_name: "Read".into(),
        content: vec![Content::Text { text: text.into() }],
        is_error: false,
        timestamp: 0,
        retention: Retention::Normal,
    })
}

/// One task, three judged calls (a, b, c) and a pinned recent call (d).
fn transcript() -> Vec<AgentMessage> {
    vec![
        user("Fix the failing test."),
        call("a"),
        result_text("a", &"old file contents ".repeat(50)),
        call("b"),
        result_text("b", &"still relevant ".repeat(50)),
        call("c"),
        result_text("c", &"stale output ".repeat(50)),
        user("continue"),
        call("d"),
        result_text("d", "recent"),
    ]
}

fn short_tail() -> PruneOptions {
    PruneOptions {
        preserve_recent: 3,
        decide_min_candidates: 1,
        ..PruneOptions::default()
    }
}

/// Keep b; keep a's call but drop its contents; remove c altogether.
fn stale_judge() -> FakeJudge {
    FakeJudge {
        call_probs: HashMap::from([
            ("result_a".into(), 0.1),
            ("call_a".into(), 0.8),
            ("result_c".into(), 0.05),
            ("call_c".into(), 0.1),
        ]),
        ..Default::default()
    }
}

async fn decide_with(
    ledger: &mut PruneLedger,
    judge: &FakeJudge,
    messages: &[AgentMessage],
    options: &PruneOptions,
) -> DecideReport {
    match ledger
        .decide(messages, judge, options, CancellationToken::new())
        .await
    {
        Ok(report) => report,
        Err(error) => panic!("decide failed: {error}"),
    }
}

fn tool_result_text(messages: &[AgentMessage], id: &str) -> Option<String> {
    messages.iter().find_map(|m| match m {
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id,
            content,
            ..
        }) if tool_call_id == id => content.iter().find_map(|c| match c {
            Content::Text { text } => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    })
}

#[tokio::test]
async fn deciding_records_verdicts_without_editing() {
    let mut ledger = PruneLedger::default();
    let messages = transcript();
    let report = decide_with(&mut ledger, &stale_judge(), &messages, &short_tail()).await;
    assert_eq!(report.verdicts.len(), 3);
    let by_id: HashMap<_, _> = report
        .verdicts
        .iter()
        .map(|v| (v.call_id.as_str(), v))
        .collect();
    assert_eq!(by_id["a"].decision, Decision::Truncate);
    assert_eq!(by_id["b"].decision, Decision::Keep);
    assert_eq!(by_id["c"].decision, Decision::Remove);
    assert_eq!(by_id["c"].keep_result, Some(0.05));
    assert_eq!(ledger.pending_count(), 2);
    assert!(ledger.pending_tokens() > 0 && report.pending_tokens == ledger.pending_tokens());
    assert_eq!(messages.len(), transcript().len(), "decide edits nothing");
}

#[tokio::test]
async fn applying_edits_once_and_clears_the_ledger() {
    let mut ledger = PruneLedger::default();
    let messages = transcript();
    decide_with(&mut ledger, &stale_judge(), &messages, &short_tail()).await;
    let (pruned, report) = ledger.apply(messages, &short_tail(), ApplyTrigger::Manual);

    assert_eq!(
        (report.removed, report.truncated, report.skipped),
        (1, 1, 0)
    );
    assert!(report.after_tokens < report.before_tokens);
    assert_eq!(
        pruned.len(),
        transcript().len() - 2,
        "c's call and result are gone"
    );
    let a = tool_result_text(&pruned, "a").unwrap_or_default();
    assert!(a.contains("more chars pruned"), "{a}");
    assert_eq!(ledger.pending_count(), 0);
}

#[tokio::test]
async fn apply_waits_for_savings_or_a_cold_cache() {
    let mut ledger = PruneLedger::default();
    let messages = transcript();
    decide_with(&mut ledger, &stale_judge(), &messages, &short_tail()).await;
    let strict = PruneOptions {
        apply_min_share: 0.99,
        ..short_tail()
    };
    let context = total_tokens(&messages);
    ledger.note_request(1_000_000);
    assert_eq!(
        ledger.apply_trigger(context, 1_000_000 + 1_000, &strict),
        None
    );
    assert_eq!(
        ledger.apply_trigger(context, 1_000_000 + strict.cache_ttl_ms, &strict),
        Some(ApplyTrigger::ColdCache)
    );
    let generous = PruneOptions {
        apply_min_share: 0.01,
        ..short_tail()
    };
    assert_eq!(
        ledger.apply_trigger(context, 1_000_000 + 1_000, &generous),
        Some(ApplyTrigger::Savings)
    );
}

#[tokio::test]
async fn a_ledger_that_never_saw_a_request_treats_the_cache_as_cold() {
    let mut ledger = PruneLedger::default();
    let messages = transcript();
    decide_with(&mut ledger, &stale_judge(), &messages, &short_tail()).await;
    let strict = PruneOptions {
        apply_min_share: 0.99,
        ..short_tail()
    };
    // No `note_request` ever: a resumed session has no warm prefix to protect.
    assert_eq!(
        ledger.apply_trigger(total_tokens(&messages), 1, &strict),
        Some(ApplyTrigger::ColdCache)
    );
}

#[tokio::test]
async fn decide_asks_about_the_oldest_candidates_first_and_caps_requests() {
    let mut ledger = PruneLedger::default();
    let messages = transcript(); // candidates a, b, c in that order
    let judge = FakeJudge {
        limits: Some(one_call_per_batch()),
        ..stale_judge()
    };
    let options = PruneOptions {
        max_requests_per_decide: 2,
        ..short_tail()
    };
    let report = decide_with(&mut ledger, &judge, &messages, &options).await;
    assert_eq!(
        report.requests, 3,
        "one user-request round, two call rounds"
    );
    assert_eq!(report.batches.len(), 2);
    let asked: Vec<&str> = report.verdicts.iter().map(|v| v.call_id.as_str()).collect();
    assert_eq!(asked, vec!["a", "b"], "c waits for the next round");
    assert!(!ledger.is_fresh());
    assert!(
        ledger.should_decide(
            &messages,
            total_tokens(&messages) + options.decide_growth_tokens,
            &options
        ),
        "c is still undecided"
    );
}

#[tokio::test]
async fn decide_is_rate_limited_by_context_growth() {
    let mut ledger = PruneLedger::default();
    let messages = transcript();
    let options = short_tail();
    let context = total_tokens(&messages);
    assert!(ledger.should_decide(&messages, context, &options));
    decide_with(&mut ledger, &stale_judge(), &messages, &options).await;
    assert!(!ledger.should_decide(&messages, context, &options));
    assert!(
        !ledger.should_decide(&messages, context + options.decide_growth_tokens, &options),
        "b was kept and a/c are pending: nothing new to ask about"
    );
}

#[tokio::test]
async fn unanswered_questions_never_delete() {
    struct Silent;
    #[async_trait]
    impl Judge for Silent {
        async fn ask(
            &self,
            _: &str,
            _: &[Question],
            _: CancellationToken,
        ) -> Result<HashMap<String, Answer>, JudgeError> {
            Ok(HashMap::new())
        }
    }
    let mut ledger = PruneLedger::default();
    let report = match ledger
        .decide(
            &transcript(),
            &Silent,
            &short_tail(),
            CancellationToken::new(),
        )
        .await
    {
        Ok(report) => report,
        Err(error) => panic!("decide failed: {error}"),
    };
    assert!(report.verdicts.iter().all(|v| v.decision == Decision::Keep));
    assert_eq!(ledger.pending_count(), 0);
}

// ------------------------------------------------------------------ state

#[tokio::test]
async fn state_shows_the_task_and_the_judged_calls_with_their_results() {
    let judge = FakeJudge::default();
    let mut ledger = PruneLedger::default();
    decide_with(&mut ledger, &judge, &transcript(), &short_tail()).await;
    let states = judge.call_states();
    assert_eq!(states.len(), 1, "a, b, c fit one batch");
    let state = &states[0];
    assert!(state.starts_with("# Task"), "{state}");
    assert!(state.contains("[0] Fix the failing test."), "{state}");
    assert!(state.contains("t0 call read"), "{state}");
    assert!(
        state.contains("t0 Read -> ok"),
        "results are shown, not just noted: {state}"
    );
    assert!(state.contains("later messages follow"), "{state}");
}

#[tokio::test]
async fn state_is_fitted_to_the_judges_request_budget() {
    let judge = FakeJudge {
        limits: Some(JudgeLimits {
            request_tokens: 700,
            max_questions: 40,
        }),
        ..Default::default()
    };
    let mut ledger = PruneLedger::default();
    let report = decide_with(&mut ledger, &judge, &transcript(), &short_tail()).await;
    for batch in &report.batches {
        assert!(
            batch.state_tokens <= 700,
            "state {} tokens for a 700-token budget",
            batch.state_tokens
        );
    }
    assert_eq!(
        report.verdicts.len(),
        3,
        "a tight budget still judges every call"
    );
}

#[tokio::test]
async fn a_batch_reads_only_its_own_stretch_of_the_conversation() {
    let judge = FakeJudge {
        limits: Some(one_call_per_batch()),
        ..Default::default()
    };
    let mut ledger = PruneLedger::default();
    let report = decide_with(&mut ledger, &judge, &transcript(), &short_tail()).await;
    assert_eq!(report.batches.len(), 3);
    let last = report
        .batches
        .iter()
        .find(|b| b.call_ids == ["c"])
        .unwrap_or_else(|| panic!("c has its own batch: {:?}", report.batches));
    assert_eq!((last.from, last.to), (5, 6), "c's call and result only");
    let state = judge
        .call_states()
        .into_iter()
        .find(|s| s.contains("(messages 5–6"))
        .unwrap_or_default();
    assert!(state.contains("t0 call"), "{state}");
    assert!(!state.contains("t1 call"), "{state}");
}

#[test]
fn judge_token_estimate_counts_cjk_by_character() {
    use evotengine::judge::estimate_tokens;
    assert_eq!(estimate_tokens("abcdefgh"), 2);
    assert_eq!(estimate_tokens("上下文裁剪"), 5);
    assert!(estimate_tokens(&"中".repeat(100_000)) > 90_000);
}

#[test]
fn judge_limits_follow_the_published_window() {
    let limits = JudgeLimits::for_window(128_000);
    assert!(limits.request_tokens > JudgeLimits::JEV.request_tokens);
    assert!(limits.request_tokens < 128_000);
    assert_eq!(JudgeLimits::default(), JudgeLimits::JEV);
}
