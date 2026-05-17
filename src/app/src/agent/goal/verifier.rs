//! Goal stop verification.

use std::future::Future;

use crate::error::Result;
use crate::types::TranscriptItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalVerdict {
    Met { reason: String },
    NotMet { reason: String },
}

impl GoalVerdict {
    pub fn reason(&self) -> &str {
        match self {
            Self::Met { reason } | Self::NotMet { reason } => reason,
        }
    }
}

pub async fn verify_goal<F, Fut>(
    condition: &str,
    transcript: &[TranscriptItem],
    verify_fn: F,
) -> Result<GoalVerdict>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<GoalVerdict>>,
{
    let prompt = build_verification_prompt(condition, transcript)?;
    verify_fn(prompt).await
}

fn build_verification_prompt(condition: &str, transcript: &[TranscriptItem]) -> Result<String> {
    let transcript_json = serde_json::to_string_pretty(transcript)?;
    Ok(format!(
        "You are verifying a stop condition. Your task is to decide whether the main agent completed the goal.\n\n\
         <goal>\n{condition}\n</goal>\n\n\
         <conversation_transcript>\n{transcript_json}\n</conversation_transcript>\n\n\
         Use the transcript as the source of truth. Inspect the codebase with tools only when the transcript is insufficient.\n\
         Return exactly one structured result with the goal_result tool:\n\
         - ok: true if the goal is complete\n\
         - ok: false with a concise reason if more work is needed"
    ))
}
