//! Judge trace — every question the judge was asked in a session, with the
//! state it read and what it answered, appended to
//! `sessions/<id>/judge-trace.jsonl`.
//!
//! The transcript keeps the judge's verdicts; this keeps its evidence. It is
//! the only place the exact state text survives, so it is what to read when
//! a verdict looks wrong: the abridging, the task header and the questions
//! are all there verbatim. Append-only, one JSON object per line, never
//! rewritten. A write failure is logged and dropped: tracing must never
//! fail a judge request.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use evot_engine::judge::Answer;
use evot_engine::judge::Judge;
use evot_engine::judge::JudgeError;
use evot_engine::judge::Question;
use evot_engine::judge::QuestionKind;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

/// Schema version of a trace line. Bump when a field changes meaning.
pub const JUDGE_TRACE_VERSION: u32 = 1;

pub const JUDGE_TRACE_FILE: &str = "judge-trace.jsonl";

/// A [`Judge`] that records each request to a file before handing back
/// the answer.
pub struct TracingJudge {
    inner: Arc<dyn Judge>,
    path: PathBuf,
}

impl TracingJudge {
    pub fn new(inner: Arc<dyn Judge>, session_dir: PathBuf) -> Self {
        Self {
            inner,
            path: session_dir.join(JUDGE_TRACE_FILE),
        }
    }
}

#[derive(Serialize)]
struct TraceLine<'a> {
    version: u32,
    ts_ms: u64,
    elapsed_ms: u64,
    state: &'a str,
    questions: Vec<TraceQuestion<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answers: Option<HashMap<&'a str, TraceAnswer<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct TraceQuestion<'a> {
    id: &'a str,
    instructions: &'a str,
    #[serde(flatten)]
    kind: TraceKind<'a>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TraceKind<'a> {
    Noul {
        #[serde(skip_serializing_if = "Option::is_none")]
        yes: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        no: Option<&'a str>,
    },
    Choice {
        options: Vec<(&'a str, Option<&'a str>)>,
    },
    Score {
        levels: Vec<&'a str>,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum TraceAnswer<'a> {
    Noul {
        probability: f64,
    },
    Choice {
        choice: &'a str,
        probabilities: &'a HashMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        probabilities: &'a [f64],
        confidence: f64,
    },
}

fn trace_question(question: &Question) -> TraceQuestion<'_> {
    let kind = match &question.kind {
        QuestionKind::Noul { yes, no } => TraceKind::Noul {
            yes: yes.as_deref(),
            no: no.as_deref(),
        },
        QuestionKind::Choice(options) => TraceKind::Choice {
            options: options
                .iter()
                .map(|(o, r)| (o.as_str(), r.as_deref()))
                .collect(),
        },
        QuestionKind::Score(levels) => TraceKind::Score {
            levels: levels.iter().map(String::as_str).collect(),
        },
    };
    TraceQuestion {
        id: &question.id,
        instructions: &question.instructions,
        kind,
    }
}

fn trace_answer(answer: &Answer) -> TraceAnswer<'_> {
    match answer {
        Answer::Noul { probability } => TraceAnswer::Noul {
            probability: *probability,
        },
        Answer::Choice {
            choice,
            probabilities,
            confidence,
        } => TraceAnswer::Choice {
            choice,
            probabilities,
            confidence: *confidence,
        },
        Answer::Score {
            score,
            probabilities,
            confidence,
        } => TraceAnswer::Score {
            score: *score,
            probabilities,
            confidence: *confidence,
        },
    }
}

impl TracingJudge {
    fn record(&self, line: &TraceLine<'_>) {
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut json = serde_json::to_vec(line).map_err(std::io::Error::other)?;
            json.push(b'\n');
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            file.write_all(&json)
        })();
        if let Err(error) = result {
            tracing::warn!(path = %self.path.display(), error = %error, "judge trace: write failed");
        }
    }
}

#[async_trait]
impl Judge for TracingJudge {
    async fn ask(
        &self,
        state: &str,
        questions: &[Question],
        cancel: CancellationToken,
    ) -> Result<HashMap<String, Answer>, JudgeError> {
        let started = std::time::Instant::now();
        let ts_ms = evot_engine::context::now_ms();
        let result = self.inner.ask(state, questions, cancel).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let line = TraceLine {
            version: JUDGE_TRACE_VERSION,
            ts_ms,
            elapsed_ms,
            state,
            questions: questions.iter().map(trace_question).collect(),
            answers: result.as_ref().ok().map(|answers| {
                answers
                    .iter()
                    .map(|(id, a)| (id.as_str(), trace_answer(a)))
                    .collect()
            }),
            error: result.as_ref().err().map(ToString::to_string),
        };
        self.record(&line);
        result
    }
}
