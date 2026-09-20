//! Prune — lossless compaction by deletion, decided by a [`Judge`].
//!
//! A summary rewrites history and loses exact paths, errors and commands. This
//! stage never rewrites: it asks the judge, per tool call, whether the call and
//! its result still matter to the ongoing task, then drops or truncates the
//! stale ones. User and assistant text is untouched. Modelled on
//! `fast-jev-compaction`.
//!
//! Deciding and applying are separate because of prompt caching. Provider
//! caches match a prefix: editing message k re-bills everything after k, and
//! one edit costs the same as a hundred. So the judge is asked often (cheap,
//! a separate request, leaves the main model's cache alone) and its verdicts
//! collect in a [`PruneLedger`]; the edits are applied in one batch when they
//! pay for the cache miss or when the cache is cold anyway.
//!
//! Deciding:
//! 1. Pair every tool call with its result. The first message and the newest
//!    `preserve_recent` messages are pinned.
//! 2. Ask which user requests are still in play. The judge reads every
//!    user request with the assistant's closing reply to it and answers one
//!    noul per request; the latest request is in play by definition. A
//!    greeting, a closed task or an abandoned direction drops out here. If
//!    this round fails, every request counts as in play.
//! 3. Split the candidates into batches of `calls_per_request`, oldest first.
//!    Each batch reads its own state: the task (the requests still in play,
//!    with their message indices) and the stretch of conversation the
//!    batch's calls live in, verbatim, results excerpted, fitted into
//!    `max_state_tokens`. The judge sees the actual content it is asked
//!    about, not a summary of it. Batches are asked concurrently.
//! 4. Two noul questions per candidate: keep the call, keep the result.
//! 5. `keep_result >= threshold` keeps both; else `keep_call >= threshold`
//!    keeps the call and truncates the result; else the pair goes.
//!
//! Applying: the ledger's pending verdicts are re-checked against the current
//! history (a call that became pinned is left alone) and applied at once.

use std::collections::HashMap;
use std::collections::HashSet;

use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::context::tokens::content_tokens;
use crate::context::tokens::total_tokens;
use crate::judge::Answer;
use crate::judge::Judge;
use crate::judge::JudgeError;
use crate::judge::Question;
use crate::types::AgentMessage;
use crate::types::Content;
use crate::types::Message;

#[derive(Debug, Clone)]
pub struct PruneOptions {
    /// Newest messages never touched (the first message is always kept).
    pub preserve_recent: usize,
    /// Minimum keep probability for a call or a result to stay.
    pub keep_threshold: f64,
    /// Budget for the state one batch reads, in (estimated) tokens. Jev reads
    /// 32k; the questions take a share and CJK text is one token a character,
    /// so the estimate is by script, not by length.
    pub max_state_tokens: usize,
    /// Judge requests in flight at once during one decide round.
    pub decide_concurrency: usize,
    /// Characters of a dropped result kept before its note.
    pub truncate_head_chars: usize,
    /// Candidate calls per judge request (two questions each).
    pub calls_per_request: usize,
    /// Ask the judge again once the context grew by this many tokens.
    pub decide_growth_tokens: usize,
    /// Do not ask for fewer undecided candidates than this.
    pub decide_min_candidates: usize,
    /// Apply when pending savings reach this share of the context: one cache
    /// miss (~0.9x the context at uncached price) pays back within a few
    /// turns of saving this much on every request.
    pub apply_min_share: f64,
    /// Apply regardless of share once the main model's cache is cold: the
    /// provider prefix cache expires after about this long without a request.
    pub cache_ttl_ms: u64,
    /// Judge requests per decide round. A resumed multi-megatoken session has
    /// thousands of candidates; the oldest are asked first and the rest wait
    /// for the next round rather than stalling the run.
    pub max_requests_per_decide: usize,
}

impl Default for PruneOptions {
    fn default() -> Self {
        Self {
            preserve_recent: 6,
            keep_threshold: 0.5,
            // ~25k tokens: under Jev's 32k state limit with room for questions.
            max_state_tokens: 16_000,
            decide_concurrency: 4,
            truncate_head_chars: 300,
            calls_per_request: 40,
            decide_growth_tokens: 15_000,
            decide_min_candidates: 8,
            apply_min_share: 0.20,
            cache_ttl_ms: 5 * 60 * 1000,
            max_requests_per_decide: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Keep,
    Truncate,
    Remove,
}

/// The judge's verdict on one tool call, with the numbers behind it so the
/// quality of the model can be evaluated afterwards.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Verdict {
    pub call_id: String,
    pub tool_name: String,
    /// Compact JSON of the call arguments, truncated for the record.
    pub arguments: String,
    pub decision: Decision,
    /// P(the call still matters); `None` when the judge did not answer.
    pub keep_call: Option<f64>,
    /// P(the verbatim result is still needed).
    pub keep_result: Option<f64>,
    /// Estimated tokens the decision frees when applied.
    pub saves_tokens: usize,
}

/// One round of asking the judge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecideReport {
    pub verdicts: Vec<Verdict>,
    pub context_tokens: usize,
    /// Tokens the pending verdicts would free, all rounds included.
    pub pending_tokens: usize,
    pub requests: usize,
    pub elapsed_ms: u64,
    /// Which user requests the judge considered still in play; the task the
    /// verdicts were judged against. Empty when the round was skipped.
    #[serde(default)]
    pub user_requests: Vec<RequestRelevance>,
    /// One entry per judge request of the call round: what stretch of the
    /// conversation it covered and how large the state was.
    #[serde(default)]
    pub batches: Vec<BatchReport>,
}

/// The judge's view of one user request when deciding what the task is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestRelevance {
    /// Index of the user message in the history.
    pub message_index: usize,
    /// The request, abridged.
    pub text: String,
    /// P(still in play). `None` when not asked: the latest request, a request
    /// too old to fit the round, or a round that failed.
    pub probability: Option<f64>,
    pub in_play: bool,
}

/// One judge request of the call round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchReport {
    /// Message range the state rendered verbatim.
    pub from: usize,
    pub to: usize,
    /// Estimated tokens of the state after fitting.
    pub state_tokens: usize,
    pub call_ids: Vec<String>,
}

/// One batch of edits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApplyReport {
    pub removed: usize,
    pub truncated: usize,
    /// Pending verdicts skipped because their call is pinned or gone.
    pub skipped: usize,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub before_messages: usize,
    pub after_messages: usize,
    pub trigger: ApplyTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyTrigger {
    /// Pending savings reached `apply_min_share` of the context.
    Savings,
    /// No request for longer than the cache TTL: the prefix is cold anyway.
    ColdCache,
    /// A summary compaction is about to rebuild the prefix regardless.
    BeforeCompaction,
    /// The user asked.
    Manual,
}

/// Decide/apply state across turns of one session.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PruneLedger {
    /// Verdicts not yet applied, by call id. Only removals and truncations
    /// are pending; a keep is final until the call is judged again.
    pending: HashMap<String, Verdict>,
    /// Calls judged `Keep` and how large the context was then; asked again
    /// once the conversation has moved on enough.
    kept_at: HashMap<String, usize>,
    /// Context size when the judge was last asked.
    decided_at_tokens: usize,
    /// Wall-clock time of the last main-model request the ledger saw.
    last_request_ms: u64,
}

impl PruneLedger {
    pub fn pending_tokens(&self) -> usize {
        self.pending.values().map(|v| v.saves_tokens).sum()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Record that the main model was just asked; feeds the cold-cache rule.
    pub fn note_request(&mut self, now_ms: u64) {
        self.last_request_ms = now_ms;
    }

    fn cache_is_cold(&self, now_ms: u64, ttl_ms: u64) -> bool {
        // No request seen in this process (a resumed session) means no warm
        // prefix to protect either.
        self.last_request_ms == 0 || now_ms.saturating_sub(self.last_request_ms) >= ttl_ms
    }

    /// Whether it is time to ask the judge again.
    pub fn should_decide(
        &self,
        messages: &[AgentMessage],
        context_tokens: usize,
        options: &PruneOptions,
    ) -> bool {
        // A ledger that has never asked may ask now; afterwards wait for growth.
        let asked_before = self.decided_at_tokens > 0;
        if asked_before
            && context_tokens
                < self
                    .decided_at_tokens
                    .saturating_add(options.decide_growth_tokens)
        {
            return false;
        }
        undecided(messages, self, options.preserve_recent).len() >= options.decide_min_candidates
    }

    /// True until the judge has been asked once in this process: a resumed
    /// session starts here even when its history is long.
    pub fn is_fresh(&self) -> bool {
        self.decided_at_tokens == 0
    }

    /// Why the pending edits should be applied now, if at all.
    pub fn apply_trigger(
        &self,
        context_tokens: usize,
        now_ms: u64,
        options: &PruneOptions,
    ) -> Option<ApplyTrigger> {
        if self.pending.is_empty() {
            return None;
        }
        let share = self.pending_tokens() as f64 / context_tokens.max(1) as f64;
        if share >= options.apply_min_share {
            return Some(ApplyTrigger::Savings);
        }
        if self.cache_is_cold(now_ms, options.cache_ttl_ms) {
            return Some(ApplyTrigger::ColdCache);
        }
        None
    }

    /// Ask the judge about every candidate not yet decided (or decided
    /// `Keep` long enough ago). Records verdicts; edits nothing.
    pub async fn decide(
        &mut self,
        messages: &[AgentMessage],
        judge: &dyn Judge,
        options: &PruneOptions,
        cancel: CancellationToken,
    ) -> Result<DecideReport, JudgeError> {
        let started = std::time::Instant::now();
        let context_tokens = total_tokens(messages);
        let mut candidates = undecided(messages, self, options.preserve_recent);
        candidates
            .truncate(options.calls_per_request.max(1) * options.max_requests_per_decide.max(1));
        self.decided_at_tokens = context_tokens;
        if candidates.is_empty() {
            return Ok(DecideReport {
                verdicts: Vec::new(),
                context_tokens,
                pending_tokens: self.pending_tokens(),
                requests: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
                user_requests: Vec::new(),
                batches: Vec::new(),
            });
        }
        let (user_requests, task_requests) =
            judge_user_requests(messages, judge, options, cancel.clone()).await?;
        let task = task_header(&user_requests);
        let mut batches = Vec::new();
        let jobs: Vec<(String, Vec<Question>)> = candidates
            .chunks(options.calls_per_request.max(1))
            .map(|batch| {
                let (state, from, to) =
                    render_state(messages, batch, &task, options.max_state_tokens);
                batches.push(BatchReport {
                    from,
                    to,
                    state_tokens: estimate_tokens(&state),
                    call_ids: batch.iter().map(|c| c.call_id.clone()).collect(),
                });
                (state, batch.iter().flat_map(questions_for).collect())
            })
            .collect();
        let requests = jobs.len() + task_requests;
        let asked: Vec<Result<HashMap<String, Answer>, JudgeError>> = futures::stream::iter(jobs)
            .map(|(state, questions)| {
                let cancel = cancel.clone();
                async move { judge.ask(&state, &questions, cancel).await }
            })
            .buffer_unordered(options.decide_concurrency.max(1))
            .collect()
            .await;
        let mut answers = HashMap::new();
        for result in asked {
            answers.extend(result?);
        }
        let verdicts: Vec<Verdict> = candidates
            .iter()
            .map(|candidate| verdict(candidate, &answers, options))
            .collect();
        for v in &verdicts {
            match v.decision {
                Decision::Keep => {
                    self.pending.remove(&v.call_id);
                    self.kept_at.insert(v.call_id.clone(), context_tokens);
                }
                Decision::Truncate | Decision::Remove => {
                    self.kept_at.remove(&v.call_id);
                    self.pending.insert(v.call_id.clone(), v.clone());
                }
            }
        }
        Ok(DecideReport {
            verdicts,
            context_tokens,
            pending_tokens: self.pending_tokens(),
            requests,
            elapsed_ms: started.elapsed().as_millis() as u64,
            user_requests,
            batches,
        })
    }

    /// Apply every pending verdict in one batch. Verdicts whose call is now
    /// pinned or no longer present are dropped without editing.
    pub fn apply(
        &mut self,
        messages: Vec<AgentMessage>,
        options: &PruneOptions,
        trigger: ApplyTrigger,
    ) -> (Vec<AgentMessage>, ApplyReport) {
        let before_tokens = total_tokens(&messages);
        let before_messages = messages.len();
        let live: HashSet<String> = collect_candidates(&messages, options.preserve_recent)
            .into_iter()
            .map(|c| c.call_id)
            .collect();
        let pending = std::mem::take(&mut self.pending);
        let mut remove = HashSet::new();
        let mut truncate = HashSet::new();
        let mut skipped = 0;
        for (id, verdict) in pending {
            if !live.contains(&id) {
                skipped += 1;
                continue;
            }
            match verdict.decision {
                Decision::Remove => remove.insert(id),
                Decision::Truncate => truncate.insert(id),
                Decision::Keep => false,
            };
        }
        let messages = edit(messages, &remove, &truncate, options.truncate_head_chars);
        // Everything judged so far is either applied or stale; the next
        // decide round looks at the history as it is now.
        self.kept_at.clear();
        self.decided_at_tokens = total_tokens(&messages);
        let report = ApplyReport {
            removed: remove.len(),
            truncated: truncate.len(),
            skipped,
            before_tokens,
            after_tokens: self.decided_at_tokens,
            before_messages,
            after_messages: messages.len(),
            trigger,
        };
        (messages, report)
    }
}

// ----------------------------------------------------------------- collect

#[derive(Debug, Clone)]
struct Candidate {
    call_id: String,
    tool_name: String,
    arguments: String,
    result_note: String,
    /// Index of the assistant message holding the call, and of its result.
    call_index: usize,
    result_index: Option<usize>,
    /// Tokens freed by removing the pair (call block + result message).
    pair_tokens: usize,
    /// Tokens freed by truncating the result to its head.
    truncation_tokens: usize,
}

fn collect_candidates(messages: &[AgentMessage], preserve_recent: usize) -> Vec<Candidate> {
    let pinned_from = messages.len().saturating_sub(preserve_recent);
    let mut results: HashMap<&str, (usize, &Message)> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if let AgentMessage::Llm(result @ Message::ToolResult { tool_call_id, .. }) = message {
            results.insert(tool_call_id.as_str(), (index, result));
        }
    }
    let mut candidates = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if index == 0 || index >= pinned_from {
            continue;
        }
        let AgentMessage::Llm(Message::Assistant { content, .. }) = message else {
            continue;
        };
        for block in content {
            let Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } = block
            else {
                continue;
            };
            let result = results.get(id.as_str()).copied();
            if result.is_some_and(|(at, _)| at >= pinned_from) {
                continue; // its result is pinned: leave the pair alone
            }
            let result_tokens = result
                .map(|(_, r)| match r {
                    Message::ToolResult { content, .. } => content_tokens(content),
                    _ => 0,
                })
                .unwrap_or(0);
            let call_tokens = content_tokens(std::slice::from_ref(block));
            candidates.push(Candidate {
                call_id: id.clone(),
                tool_name: name.clone(),
                arguments: compact_json(arguments),
                result_note: result
                    .map(|(_, r)| result_note(r))
                    .unwrap_or_else(|| "no result".into()),
                call_index: index,
                result_index: result.map(|(at, _)| at),
                pair_tokens: call_tokens + result_tokens,
                truncation_tokens: result_tokens.saturating_sub(result_tokens.min(100)),
            });
        }
    }
    candidates
}

/// Candidates the ledger has no live verdict for. A `Keep` is revisited once
/// the context has grown by a decide window since it was given.
fn undecided(
    messages: &[AgentMessage],
    ledger: &PruneLedger,
    preserve_recent: usize,
) -> Vec<Candidate> {
    let now = total_tokens(messages);
    collect_candidates(messages, preserve_recent)
        .into_iter()
        .filter(|c| !ledger.pending.contains_key(&c.call_id))
        .filter(|c| match ledger.kept_at.get(&c.call_id) {
            Some(at) => now >= at.saturating_add(ledger_revisit_tokens()),
            None => true,
        })
        .collect()
}

fn ledger_revisit_tokens() -> usize {
    40_000
}

fn result_note(message: &Message) -> String {
    let Message::ToolResult {
        content, is_error, ..
    } = message
    else {
        return String::new();
    };
    let chars: usize = content.iter().map(block_chars).sum();
    format!(
        "{}, {chars} chars (omitted)",
        if *is_error { "error" } else { "ok" }
    )
}

fn block_chars(block: &Content) -> usize {
    match block {
        Content::Text { text } => text.chars().count(),
        Content::Thinking { thinking, .. } => thinking.chars().count(),
        Content::ToolCall { arguments, .. } => arguments.to_string().len(),
        Content::Image { .. } => 0,
    }
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

// ---------------------------------------------------------- user requests

/// User requests judged per round. Older ones are out of play by age.
const MAX_REQUESTS_JUDGED: usize = 40;
/// Characters of a request shown to the judge.
const REQUEST_CHARS: usize = 300;
/// Characters of the assistant's closing reply shown next to a request.
const CLOSING_CHARS: usize = 200;
/// Characters of a request kept in the task header and the report.
const TASK_REQUEST_CHARS: usize = 300;

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
/// failed round degrades to "everything is in play" rather than blocking
/// the call round.
async fn judge_user_requests(
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
            tracing::warn!(error = %error, "judge prune: user request round failed; treating every request as in play");
            for entry in report.iter_mut() {
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

// ------------------------------------------------------------------- state

/// The task, once per batch: the user requests still in play, with their
/// message indices so the judge can tell which one a stretch of
/// conversation served. What "still matters" is judged against these.
fn task_header(requests: &[RequestRelevance]) -> String {
    let mut lines =
        vec!["# Task (user requests still in play, message index in brackets)".to_string()];
    for request in requests.iter().filter(|r| r.in_play) {
        lines.push(format!("[{}] {}", request.message_index, request.text));
    }
    lines.join("\n")
}

/// The state one batch reads: the task, then the stretch of conversation from
/// the batch's first call to its last result — verbatim, tool results
/// excerpted — fitted into `budget` tokens, oldest lines abridged first. A
/// trailing line says how much conversation follows, so "later superseded"
/// is answerable. Returns the state and the message range it rendered.
fn render_state(
    messages: &[AgentMessage],
    batch: &[Candidate],
    task: &str,
    budget: usize,
) -> (String, usize, usize) {
    let tags: HashMap<&str, usize> = batch
        .iter()
        .enumerate()
        .map(|(n, c)| (c.call_id.as_str(), n))
        .collect();
    let from = batch.iter().map(|c| c.call_index).min().unwrap_or(0);
    let to = batch
        .iter()
        .map(|c| c.result_index.unwrap_or(c.call_index))
        .max()
        .unwrap_or(from)
        .min(messages.len().saturating_sub(1));
    let mut entries: Vec<String> = messages[from..=to]
        .iter()
        .filter_map(|message| render_entry(message, &tags))
        .collect();
    let after = messages.len().saturating_sub(to + 1);
    let head = format!("{task}\n\n# Conversation (messages {from}–{to} of {}; the calls to judge are tagged t0, t1, …)", messages.len());
    let tail = if after > 0 {
        format!("[… {after} later messages follow, up to the present …]")
    } else {
        "[end of conversation]".to_string()
    };
    fit(
        &mut entries,
        budget.saturating_sub(estimate_tokens(&head) + estimate_tokens(&tail)),
    );
    (format!("{head}\n{}\n{tail}", entries.join("\n")), from, to)
}

fn render_entry(message: &AgentMessage, tags: &HashMap<&str, usize>) -> Option<String> {
    let AgentMessage::Llm(message) = message else {
        return None;
    };
    match message {
        Message::User { content, .. } => Some(format!("user: {}", text_of(content))),
        Message::Assistant { content, .. } => {
            let mut lines = Vec::new();
            let text = text_of(content);
            if !text.is_empty() {
                lines.push(format!("assistant: {text}"));
            }
            for block in content {
                if let Content::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } = block
                {
                    let tag = tags
                        .get(id.as_str())
                        .map(|n| format!("t{n} "))
                        .unwrap_or_default();
                    lines.push(format!("{tag}call {name} {}", compact_json(arguments)));
                }
            }
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
        Message::ToolResult {
            tool_call_id,
            tool_name,
            content,
            is_error,
            ..
        } => {
            let tag = tags
                .get(tool_call_id.as_str())
                .map(|n| format!("t{n} "))
                .unwrap_or_default();
            let body = text_of(content);
            let status = if *is_error { "error" } else { "ok" };
            Some(format!(
                "{tag}{tool_name} -> {status}\n{}",
                abridge(&body, RESULT_EXCERPT_CHARS)
            ))
        }
    }
}

/// Characters of a tool result shown to the judge before abridging.
const RESULT_EXCERPT_CHARS: usize = 1_600;

fn text_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tokens, estimated by script: a CJK character is about one token, other
/// text about four characters a token. Deliberately pessimistic.
pub(crate) fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        let c = ch as u32;
        let is_cjk = (0x3000..=0x9FFF).contains(&c)
            || (0xAC00..=0xD7AF).contains(&c)
            || (0xF900..=0xFAFF).contains(&c)
            || (0xFF00..=0xFFEF).contains(&c)
            || (0x20000..=0x3134F).contains(&c);
        if is_cjk {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

/// Abridge oldest entries first until the state fits: long entries to head +
/// tail, then whole old entries to a note. The newest quarter keeps its text;
/// tagged entries (the ones being judged) keep at least a head.
fn fit(entries: &mut [String], budget: usize) {
    let total = |entries: &[String]| {
        entries
            .iter()
            .map(|e| estimate_tokens(e) + 1)
            .sum::<usize>()
    };
    if total(entries) <= budget {
        return;
    }
    let abridgeable = entries.len() - entries.len() / 4;
    for cap in [1000usize, 400, 160, 60] {
        for entry in entries.iter_mut().take(abridgeable) {
            if entry.chars().count() > cap {
                *entry = abridge(entry, cap);
            }
        }
        if total(entries) <= budget {
            return;
        }
    }
    for cap in [400usize, 160, 60] {
        for entry in entries.iter_mut() {
            if entry.chars().count() > cap {
                *entry = abridge(entry, cap);
            }
        }
        if total(entries) <= budget {
            return;
        }
    }
    for entry in entries.iter_mut().take(abridgeable) {
        if !entry.starts_with('t') {
            *entry = format!("[… {} chars omitted …]", entry.chars().count());
        }
    }
}

fn abridge(text: &str, cap: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= cap {
        return text.to_string();
    }
    let head: String = chars[..cap * 2 / 3].iter().collect();
    let tail: String = chars[chars.len() - cap / 3..].iter().collect();
    format!("{head} […] {tail}")
}

// --------------------------------------------------------------- questions

/// Characters of the call arguments quoted in a question. The question only
/// has to identify the call; the state carries the arguments (fitted to the
/// budget). Unbounded, a `Write` of a large file made one question larger than
/// Jev's window and the whole batch failed with `max_tokens_exceeded`.
const QUESTION_ARGUMENT_CHARS: usize = 240;

fn questions_for(candidate: &Candidate) -> Vec<Question> {
    let (call_id, result_id) = question_ids(candidate);
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

fn question_ids(candidate: &Candidate) -> (String, String) {
    let key: String = candidate
        .call_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    (format!("call_{key}"), format!("result_{key}"))
}

fn verdict(
    candidate: &Candidate,
    answers: &HashMap<String, Answer>,
    options: &PruneOptions,
) -> Verdict {
    let (call_id, result_id) = question_ids(candidate);
    let keep_call = answers.get(&call_id).and_then(Answer::probability);
    let keep_result = answers.get(&result_id).and_then(Answer::probability);
    let threshold = options.keep_threshold;
    let decision = match (keep_call, keep_result) {
        (_, Some(result)) if result >= threshold => Decision::Keep,
        (Some(call), _) if call >= threshold => Decision::Truncate,
        (None, None) => Decision::Keep, // unanswered: never delete on silence
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

// ------------------------------------------------------------------- apply

fn edit(
    messages: Vec<AgentMessage>,
    remove: &HashSet<String>,
    truncate: &HashSet<String>,
    head_chars: usize,
) -> Vec<AgentMessage> {
    if remove.is_empty() && truncate.is_empty() {
        return messages;
    }
    messages
        .into_iter()
        .filter_map(|message| match message {
            AgentMessage::Llm(Message::Assistant {
                content,
                stop_reason,
                model,
                provider,
                usage,
                timestamp,
                error_message,
                response_id,
            }) => {
                let content: Vec<Content> = content
                    .into_iter()
                    .filter(|block| !matches!(block, Content::ToolCall { id, .. } if remove.contains(id)))
                    .collect();
                // A reply that only carried removed calls has nothing left to say.
                (!content.is_empty()).then(|| {
                    AgentMessage::Llm(Message::Assistant {
                        content,
                        stop_reason,
                        model,
                        provider,
                        usage,
                        timestamp,
                        error_message,
                        response_id,
                    })
                })
            }
            AgentMessage::Llm(Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
                timestamp,
                retention,
            }) => {
                if remove.contains(&tool_call_id) {
                    return None;
                }
                let content = if truncate.contains(&tool_call_id) {
                    truncate_result(content, head_chars)
                } else {
                    content
                };
                Some(AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    tool_name,
                    content,
                    is_error,
                    timestamp,
                    retention,
                }))
            }
            other => Some(other),
        })
        .collect()
}

fn truncate_result(content: Vec<Content>, head_chars: usize) -> Vec<Content> {
    let full = text_of(&content);
    let total = full.chars().count();
    if total <= head_chars {
        return content;
    }
    let head: String = full.chars().take(head_chars).collect();
    vec![Content::Text {
        text: format!(
            "{head}\n[… {} more chars pruned: no longer needed for the current task …]",
            total - head_chars
        ),
    }]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::types::Retention;
    use crate::types::StopReason;
    use crate::types::Usage;

    struct Scripted(HashMap<String, f64>);

    #[async_trait]
    impl Judge for Scripted {
        async fn ask(
            &self,
            _state: &str,
            questions: &[Question],
            _cancel: CancellationToken,
        ) -> Result<HashMap<String, Answer>, JudgeError> {
            Ok(questions
                .iter()
                .map(|q| {
                    let p = *self.0.get(&q.id).unwrap_or(&0.9);
                    (q.id.clone(), Answer::Noul { probability: p })
                })
                .collect())
        }
    }

    fn user(text: &str) -> AgentMessage {
        AgentMessage::Llm(Message::user(text))
    }

    fn call(id: &str, name: &str) -> AgentMessage {
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: json!({"path": format!("{id}.txt")}),
                metadata: None,
            }],
            stop_reason: StopReason::ToolUse,
            model: "m".into(),
            provider: "p".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        })
    }

    fn result(id: &str, text: &str) -> AgentMessage {
        AgentMessage::Llm(Message::ToolResult {
            tool_call_id: id.into(),
            tool_name: "Read".into(),
            content: vec![Content::Text { text: text.into() }],
            is_error: false,
            timestamp: 0,
            retention: Retention::default(),
        })
    }

    fn transcript() -> Vec<AgentMessage> {
        vec![
            user("Fix the failing test."),
            call("a", "Read"),
            result("a", &"old file contents ".repeat(50)),
            call("b", "Read"),
            result("b", &"still relevant ".repeat(50)),
            call("c", "Bash"),
            result("c", &"stale output ".repeat(50)),
            user("continue"),
            call("d", "Read"),
            result("d", "recent"),
        ]
    }

    fn options() -> PruneOptions {
        PruneOptions {
            preserve_recent: 3,
            decide_min_candidates: 1,
            ..PruneOptions::default()
        }
    }

    fn task() -> String {
        task_header(&[RequestRelevance {
            message_index: 0,
            text: "Fix the failing test.".into(),
            probability: None,
            in_play: true,
        }])
    }

    fn stale_judge() -> Scripted {
        Scripted(HashMap::from([
            ("result_a".into(), 0.1),
            ("call_a".into(), 0.8), // keep the call, drop the contents
            ("result_c".into(), 0.05),
            ("call_c".into(), 0.1), // remove the pair
        ]))
    }

    #[tokio::test]
    async fn deciding_records_verdicts_without_editing() {
        let mut ledger = PruneLedger::default();
        let messages = transcript();
        let report = ledger
            .decide(
                &messages,
                &stale_judge(),
                &options(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
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
        assert_eq!(messages.len(), transcript().len()); // untouched
    }

    #[tokio::test]
    async fn applying_edits_once_and_clears_the_ledger() {
        let mut ledger = PruneLedger::default();
        let messages = transcript();
        ledger
            .decide(
                &messages,
                &stale_judge(),
                &options(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let (pruned, report) = ledger.apply(messages, &options(), ApplyTrigger::Manual);

        assert_eq!(
            (report.removed, report.truncated, report.skipped),
            (1, 1, 0)
        );
        assert!(report.after_tokens < report.before_tokens);
        assert_eq!(pruned.len(), transcript().len() - 2); // c's call and result are gone
        let a = pruned
            .iter()
            .find_map(|m| match m {
                AgentMessage::Llm(Message::ToolResult {
                    tool_call_id,
                    content,
                    ..
                }) if tool_call_id == "a" => Some(text_of(content)),
                _ => None,
            })
            .unwrap();
        assert!(a.contains("more chars pruned"));
        assert_eq!(ledger.pending_count(), 0);
    }

    #[tokio::test]
    async fn apply_waits_for_savings_or_a_cold_cache() {
        let mut ledger = PruneLedger::default();
        let messages = transcript();
        ledger
            .decide(
                &messages,
                &stale_judge(),
                &options(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let opts = PruneOptions {
            apply_min_share: 0.99,
            ..options()
        };
        ledger.note_request(1_000_000);
        assert_eq!(
            ledger.apply_trigger(total_tokens(&messages), 1_000_000 + 1_000, &opts),
            None
        );
        assert_eq!(
            ledger.apply_trigger(
                total_tokens(&messages),
                1_000_000 + opts.cache_ttl_ms,
                &opts
            ),
            Some(ApplyTrigger::ColdCache)
        );
        let generous = PruneOptions {
            apply_min_share: 0.01,
            ..options()
        };
        assert_eq!(
            ledger.apply_trigger(total_tokens(&messages), 1_000_000 + 1_000, &generous),
            Some(ApplyTrigger::Savings)
        );
    }

    #[tokio::test]
    async fn decide_asks_about_the_oldest_candidates_first_and_caps_requests() {
        let mut ledger = PruneLedger::default();
        let messages = transcript(); // candidates a, b, c in that order
        let opts = PruneOptions {
            calls_per_request: 1,
            max_requests_per_decide: 2,
            ..options()
        };
        let report = ledger
            .decide(&messages, &stale_judge(), &opts, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            report.requests, 3,
            "one user-request round, two call rounds"
        );
        assert_eq!(report.batches.len(), 2);
        let asked: Vec<&str> = report.verdicts.iter().map(|v| v.call_id.as_str()).collect();
        assert_eq!(asked, vec!["a", "b"], "c waits for the next round");
        assert!(!ledger.is_fresh());
        assert_eq!(undecided(&messages, &ledger, opts.preserve_recent).len(), 1);
    }

    #[test]
    fn a_ledger_that_never_saw_a_request_treats_the_cache_as_cold() {
        let mut ledger = PruneLedger::default();
        let opts = options();
        assert!(ledger.cache_is_cold(1, opts.cache_ttl_ms));
        ledger.note_request(1_000);
        assert!(!ledger.cache_is_cold(1_000 + opts.cache_ttl_ms / 2, opts.cache_ttl_ms));
        assert!(ledger.cache_is_cold(1_000 + opts.cache_ttl_ms, opts.cache_ttl_ms));
    }

    #[tokio::test]
    async fn decide_is_rate_limited_by_context_growth() {
        let mut ledger = PruneLedger::default();
        let messages = transcript();
        let opts = options();
        assert!(ledger.should_decide(&messages, total_tokens(&messages), &opts));
        ledger
            .decide(&messages, &stale_judge(), &opts, CancellationToken::new())
            .await
            .unwrap();
        assert!(!ledger.should_decide(&messages, total_tokens(&messages), &opts));
        assert!(
            !ledger.should_decide(
                &messages,
                total_tokens(&messages) + opts.decide_growth_tokens,
                &opts
            ),
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
        let report = ledger
            .decide(&transcript(), &Silent, &options(), CancellationToken::new())
            .await
            .unwrap();
        assert!(report.verdicts.iter().all(|v| v.decision == Decision::Keep));
        assert_eq!(ledger.pending_count(), 0);
    }

    #[test]
    fn state_names_candidates_shows_their_results_and_fits_the_budget() {
        let messages = transcript();
        let candidates = collect_candidates(&messages, 3);
        let (state, from, to) = render_state(&messages, &candidates, &task(), 400);
        assert_eq!((from, to), (1, 6));
        assert!(state.starts_with("# Task"), "{state}");
        assert!(state.contains("t0 call Read"));
        assert!(
            state.contains("t0 Read -> ok"),
            "results are shown, not just noted: {state}"
        );
        assert!(state.contains("later messages follow"), "{state}");
        assert!(
            estimate_tokens(&state) <= 400 + 60,
            "{}",
            estimate_tokens(&state)
        );
    }

    #[test]
    fn a_batch_reads_only_its_own_stretch_of_the_conversation() {
        let messages = transcript();
        let candidates = collect_candidates(&messages, 3);
        assert!(candidates.len() >= 2);
        let last = &candidates[candidates.len() - 1..];
        let (state, _, _) = render_state(&messages, last, &task(), 4_000);
        assert!(state.contains("t0 call"), "{state}");
        assert!(!state.contains("t1 call"));
        // Earlier calls are outside the window, so the window starts after message 0.
        assert!(!state.contains("(messages 0–"), "{state}");
    }

    #[test]
    fn token_estimate_counts_cjk_by_character() {
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens("上下文裁剪"), 5);
        assert!(estimate_tokens(&"中".repeat(100_000)) > 90_000);
    }
}
