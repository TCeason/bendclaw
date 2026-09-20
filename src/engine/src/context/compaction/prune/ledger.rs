//! The ledger: verdicts collected across turns, and the decide / apply steps
//! that fill and drain it.

use std::collections::HashMap;
use std::collections::HashSet;

use futures::StreamExt;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::candidates;
use super::candidates::Candidate;
use super::edit;
use super::options::PruneOptions;
use super::questions;
use super::report::ApplyReport;
use super::report::ApplyTrigger;
use super::report::BatchReport;
use super::report::DecideReport;
use super::report::Decision;
use super::report::Verdict;
use super::requests;
use super::state;
use crate::context::tokens::total_tokens;
use crate::judge::Answer;
use crate::judge::Judge;
use crate::judge::JudgeError;
use crate::judge::Question;
use crate::types::AgentMessage;

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

/// A batch's answers, or why it produced none.
type Answers = Result<HashMap<String, Answer>, JudgeError>;

/// One judge request of the call round, before it is sent.
struct Batch {
    state: String,
    questions: Vec<Question>,
    call_ids: Vec<String>,
    from: usize,
    to: usize,
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
        self.undecided(messages, options).len() >= options.decide_min_candidates
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

    /// Candidates the ledger has no live verdict for. A `Keep` is revisited
    /// once the context has grown by `revisit_kept_tokens` since it was given.
    fn undecided(&self, messages: &[AgentMessage], options: &PruneOptions) -> Vec<Candidate> {
        let now = total_tokens(messages);
        candidates::collect(messages, options.preserve_recent)
            .into_iter()
            .filter(|c| !self.pending.contains_key(&c.call_id))
            .filter(|c| match self.kept_at.get(&c.call_id) {
                Some(at) => now >= at.saturating_add(options.revisit_kept_tokens),
                None => true,
            })
            .collect()
    }

    /// Ask the judge about every candidate not yet decided (or decided
    /// `Keep` long enough ago). Records verdicts; edits nothing.
    ///
    /// Batches are sized to the judge's own limits and asked concurrently.
    /// A batch that fails leaves its calls undecided for the next round; only
    /// a round in which every batch failed is an error.
    pub async fn decide(
        &mut self,
        messages: &[AgentMessage],
        judge: &dyn Judge,
        options: &PruneOptions,
        cancel: CancellationToken,
    ) -> Result<DecideReport, JudgeError> {
        let started = std::time::Instant::now();
        let context_tokens = total_tokens(messages);
        let limits = judge.limits();
        let calls_per_request = (limits.max_questions / questions::PER_CANDIDATE).max(1);
        let mut candidates = self.undecided(messages, options);
        candidates.truncate(calls_per_request * options.max_requests_per_decide.max(1));
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
            requests::judge_user_requests(messages, judge, options, cancel.clone()).await?;
        let task = requests::task_header(&user_requests);

        let batches: Vec<Batch> = candidates
            .chunks(calls_per_request)
            .map(|batch| {
                // The questions ride in the same request as the state, so
                // they come out of the same budget.
                let questions: Vec<Question> =
                    batch.iter().flat_map(questions::for_candidate).collect();
                let budget = limits
                    .request_tokens
                    .saturating_sub(questions::tokens(&questions, judge));
                let (state, from, to) = state::render(messages, batch, &task, budget, judge);
                Batch {
                    state,
                    questions,
                    call_ids: batch.iter().map(|c| c.call_id.clone()).collect(),
                    from,
                    to,
                }
            })
            .collect();
        let requests = batches.len() + task_requests;

        let asked: Vec<(BatchReport, Answers)> = futures::stream::iter(batches)
            .map(|batch| {
                let cancel = cancel.clone();
                async move {
                    let report = BatchReport {
                        from: batch.from,
                        to: batch.to,
                        state_tokens: judge.estimate_tokens(&batch.state),
                        call_ids: batch.call_ids,
                        failed: None,
                    };
                    let answers = judge.ask(&batch.state, &batch.questions, cancel).await;
                    (report, answers)
                }
            })
            .buffer_unordered(options.decide_concurrency.max(1))
            .collect()
            .await;

        let mut answers = HashMap::new();
        let mut reports = Vec::with_capacity(asked.len());
        let mut failed: HashSet<String> = HashSet::new();
        let mut last_error = None;
        for (mut report, result) in asked {
            match result {
                Ok(batch_answers) => answers.extend(batch_answers),
                Err(JudgeError::Cancelled) => return Err(JudgeError::Cancelled),
                Err(error) => {
                    tracing::warn!(
                        calls = report.call_ids.len(),
                        state_tokens = report.state_tokens,
                        error = %error,
                        "judge prune: batch failed"
                    );
                    report.failed = Some(error.to_string());
                    failed.extend(report.call_ids.iter().cloned());
                    last_error = Some(error);
                }
            }
            reports.push(report);
        }
        if let Some(error) = last_error {
            if failed.len() == candidates.len() {
                return Err(error);
            }
        }
        reports.sort_by_key(|r| r.from);

        let verdicts: Vec<Verdict> = candidates
            .iter()
            .filter(|c| !failed.contains(&c.call_id))
            .map(|c| questions::verdict(c, &answers, options))
            .collect();
        for verdict in &verdicts {
            match verdict.decision {
                Decision::Keep => {
                    self.pending.remove(&verdict.call_id);
                    self.kept_at.insert(verdict.call_id.clone(), context_tokens);
                }
                Decision::Truncate | Decision::Remove => {
                    self.kept_at.remove(&verdict.call_id);
                    self.pending
                        .insert(verdict.call_id.clone(), verdict.clone());
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
            batches: reports,
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
        let live: HashSet<String> = candidates::collect(&messages, options.preserve_recent)
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
        let messages = edit::apply(messages, &remove, &truncate, options.truncate_head_chars);
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
