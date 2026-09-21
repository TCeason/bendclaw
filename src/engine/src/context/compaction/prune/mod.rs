//! Prune — lossless compaction by deletion, decided by a [`Judge`].
//!
//! A summary rewrites history and loses exact paths, errors and commands. This
//! stage never rewrites: it asks the judge, per tool call, whether the call and
//! its result still matter to the ongoing task, then drops or truncates the
//! stale ones. User and assistant text is untouched. Modelled on
//! `fast-jev-compaction`.
//!
//! Deciding and applying are separate steps of a [`PruneLedger`]. The judge
//! is asked whenever the context has grown enough since the last round (a
//! separate request; the main model's cache is untouched) and its verdicts
//! collect as pending edits. Pending edits are applied at every run end and,
//! once the context is past the prune threshold, after each response — a
//! context kept trimmed is worth the cache miss an edit costs.
//!
//! Deciding:
//! 1. `candidates` — pair every tool call with its result. The first message
//!    and the newest `preserve_recent` messages are pinned.
//! 2. `requests` — ask which user requests are still in play. The judge reads
//!    every user request with the assistant's closing reply to it and answers
//!    one noul per request; the latest request is in play by definition. If
//!    this round fails, every request counts as in play.
//! 3. `state` — split the candidates into batches of `calls_per_request`,
//!    oldest first. Each batch reads its own state: the task (the requests
//!    still in play) and the stretch of conversation the batch's calls live
//!    in, verbatim, results excerpted, fitted with the batch's questions into
//!    `max_state_tokens`. Batches are asked concurrently; one failing batch
//!    leaves its calls undecided and does not void the others.
//! 4. `questions` — two noul questions per candidate: keep the call, keep the
//!    result. `keep_result >= threshold` keeps both; else `keep_call >=
//!    threshold` keeps the call and truncates the result; else the pair goes.
//!
//! Applying (`edit`): the ledger's pending verdicts are re-checked against the
//! current history (a call that became pinned is left alone) and applied at
//! once.
//!
//! [`Judge`]: crate::judge::Judge

mod candidates;
mod edit;
mod ledger;
mod options;
mod questions;
mod report;
mod requests;
mod state;

pub use ledger::PruneLedger;
pub use options::PruneOptions;
pub use report::ApplyReport;
pub use report::ApplyTrigger;
pub use report::BatchReport;
pub use report::DecideReport;
pub use report::Decision;
pub use report::RequestRelevance;
pub use report::Verdict;
