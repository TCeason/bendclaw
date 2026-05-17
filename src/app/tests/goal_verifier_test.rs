//! Tests for goal stop verification.

use evot::agent::goal::verify_goal;
use evot::agent::goal::GoalVerdict;
use evot::types::TranscriptItem;

#[tokio::test]
async fn verify_goal_passes_structured_verdict_through() {
    let transcript = vec![assistant_item("tests pass")];

    let verdict = verify_goal("all tests pass", &transcript, |prompt| async move {
        assert!(prompt.contains("<goal>\nall tests pass\n</goal>"));
        assert!(prompt.contains("tests pass"));
        Ok(GoalVerdict::Met {
            reason: "verified".into(),
        })
    })
    .await;

    match verdict {
        Ok(GoalVerdict::Met { reason }) => assert_eq!(reason, "verified"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[tokio::test]
async fn verify_goal_allows_not_met() {
    let transcript = vec![assistant_item("still editing")];

    let verdict = verify_goal("feature complete", &transcript, |_prompt| async {
        Ok(GoalVerdict::NotMet {
            reason: "tests not run".into(),
        })
    })
    .await;

    match verdict {
        Ok(GoalVerdict::NotMet { reason }) => assert_eq!(reason, "tests not run"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

fn assistant_item(text: &str) -> TranscriptItem {
    TranscriptItem::Assistant {
        text: text.to_string(),
        thinking: None,
        tool_calls: Vec::new(),
        stop_reason: "end_turn".into(),
    }
}
