use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

use crate::llm::message::ChatMessage;
use crate::llm::provider::LLMProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunRisk {
    ReadOnly,
    Mutating,
    Destructive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRelation {
    Append,
    Revise,
    ForkOrAsk,
}

#[derive(Debug, Clone)]
pub struct RunSnapshot {
    pub session_id: String,
    pub run_id: String,
    pub summary: String,
    pub risk: RunRisk,
    pub target_scope: Option<String>,
    pub started_at: Instant,
}

impl RunSnapshot {
    pub fn from_input(session_id: &str, run_id: &str, input: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            summary: input.chars().take(200).collect(),
            risk: RunRisk::ReadOnly,
            target_scope: None,
            started_at: Instant::now(),
        }
    }
}

#[async_trait]
pub trait TurnRelationClassifier: Send + Sync {
    async fn classify(
        &self,
        llm: &Arc<dyn LLMProvider>,
        model: &str,
        snapshot: &RunSnapshot,
        new_input: &str,
    ) -> TurnRelation;
}

/// Phase 1 stub: always returns ForkOrAsk (fail-safe default).
pub struct StubClassifier;

#[async_trait]
impl TurnRelationClassifier for StubClassifier {
    async fn classify(
        &self,
        _llm: &Arc<dyn LLMProvider>,
        _model: &str,
        _snapshot: &RunSnapshot,
        _new_input: &str,
    ) -> TurnRelation {
        TurnRelation::ForkOrAsk
    }
}

/// LLM-powered classifier. Calls the LLM with a structured prompt and parses
/// the single-word response. Falls back to ForkOrAsk on any error.
pub struct LLMClassifier;

#[async_trait]
impl TurnRelationClassifier for LLMClassifier {
    async fn classify(
        &self,
        llm: &Arc<dyn LLMProvider>,
        model: &str,
        snapshot: &RunSnapshot,
        new_input: &str,
    ) -> TurnRelation {
        let prompt = build_classification_prompt(&snapshot.summary, new_input);
        let messages = vec![ChatMessage::user(prompt)];
        match llm.chat(model, &messages, &[], 0.0).await {
            Ok(resp) => parse_relation(resp.content.as_deref().unwrap_or("")),
            Err(_) => TurnRelation::ForkOrAsk,
        }
    }
}

fn build_classification_prompt(active_summary: &str, new_input: &str) -> String {
    format!(
        "Classify the relationship between a new user message and an active task.\n\
        \n\
        Active task: \"{active_summary}\"\n\
        New message: \"{new_input}\"\n\
        \n\
        Choose exactly one:\n\
        - append: adds detail or a follow-up step without changing the task boundary\n\
        - revise: changes the scope, constraints, filters, or goal of the active task\n\
        - fork: unrelated to the active task or the relationship is ambiguous\n\
        \n\
        Respond with exactly one word: append, revise, or fork."
    )
}

fn parse_relation(text: &str) -> TurnRelation {
    let lower = text.trim().to_lowercase();
    if lower.starts_with("append") || lower == "append" {
        TurnRelation::Append
    } else if lower.starts_with("revise") || lower == "revise" {
        TurnRelation::Revise
    } else {
        TurnRelation::ForkOrAsk
    }
}
