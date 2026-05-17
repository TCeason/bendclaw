//! Tests for goal evaluator response parsing.

use evot::agent::goal::evaluator::parse_eval_response;
use evot::agent::goal::EvalVerdict;

#[test]
fn parse_met_response() {
    let raw = r#"{"status": "met", "reason": "all tests pass"}"#;
    match parse_eval_response(raw) {
        EvalVerdict::Met { reasoning } => assert_eq!(reasoning, "all tests pass"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn parse_continue_response() {
    let raw = r#"{"status": "continue", "reason": "still working"}"#;
    match parse_eval_response(raw) {
        EvalVerdict::Continue => {}
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn parse_impossible_response() {
    let raw = r#"{"status": "impossible", "reason": "missing access"}"#;
    match parse_eval_response(raw) {
        EvalVerdict::Impossible { reasoning } => assert_eq!(reasoning, "missing access"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn parse_subagent_met_response() {
    let raw = r#"{"status": "met", "reason": "verified"}"#;
    match parse_eval_response(raw) {
        EvalVerdict::Met { reasoning } => assert_eq!(reasoning, "verified"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn parse_with_markdown_fence() {
    let raw = "```json\n{\"status\": \"met\", \"reason\": \"done\"}\n```";
    match parse_eval_response(raw) {
        EvalVerdict::Met { reasoning } => assert_eq!(reasoning, "done"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}
