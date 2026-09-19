//! Judge — a model that answers typed questions about a state instead of
//! generating text (TypeSafe Jev behind llmproxy).
//!
//! Code owns the workflow; the judge supplies one calibrated decision at a
//! time: is this true (`Noul`), which of these (`Choice`), how much on this
//! rubric (`Score`). Callers get probabilities back and apply their own
//! thresholds. The abstraction is small on purpose so tests use a fake and
//! the transport can change without touching a caller.
//!
//! `wire` turns questions into the tool schema llmproxy's Jev channel expects
//! and reads answers back; `provider` sends that through an ordinary
//! [`StreamProvider`], so no separate HTTP client exists.

mod provider;
pub mod relevance;
mod wire;

use std::collections::HashMap;

use async_trait::async_trait;
pub use provider::ProviderJudge;
use tokio_util::sync::CancellationToken;

/// One typed question. `id` is the key the answer comes back under; keep it
/// short and stable (it is also the tool property name on the wire).
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub id: String,
    pub instructions: String,
    pub kind: QuestionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuestionKind {
    /// Yes / no; the answer is P(yes). Optional rubric for each side.
    Noul {
        yes: Option<String>,
        no: Option<String>,
    },
    /// One option out of a fixed set; option -> optional rubric.
    Choice(Vec<(String, Option<String>)>),
    /// A position on an ordered rubric, lowest level first.
    Score(Vec<String>),
}

impl Question {
    pub fn noul(id: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            instructions: instructions.into(),
            kind: QuestionKind::Noul {
                yes: None,
                no: None,
            },
        }
    }

    pub fn with_criteria(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        if let QuestionKind::Noul { .. } = self.kind {
            self.kind = QuestionKind::Noul {
                yes: Some(yes.into()),
                no: Some(no.into()),
            };
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Noul {
        probability: f64,
    },
    Choice {
        choice: String,
        probabilities: HashMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        probabilities: Vec<f64>,
        confidence: f64,
    },
}

impl Answer {
    /// P(yes) for a noul answer; `None` for the other kinds.
    pub fn probability(&self) -> Option<f64> {
        match self {
            Self::Noul { probability } => Some(*probability),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JudgeError {
    #[error("judge request failed: {0}")]
    Transport(String),
    #[error("judge answer unreadable: {0}")]
    Malformed(String),
    #[error("judge request cancelled")]
    Cancelled,
}

/// Ask several questions about one state in a single round trip.
#[async_trait]
pub trait Judge: Send + Sync {
    async fn ask(
        &self,
        state: &str,
        questions: &[Question],
        cancel: CancellationToken,
    ) -> Result<HashMap<String, Answer>, JudgeError>;
}
