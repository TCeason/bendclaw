//! Goal evaluator.

use crate::error::Result;

#[derive(Debug, Clone)]
pub enum EvalVerdict {
    Met { reasoning: String },
    Impossible { reasoning: String },
    Continue,
}

impl EvalVerdict {
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Met { reasoning } | Self::Impossible { reasoning } => Some(reasoning),
            Self::Continue => None,
        }
    }
}

pub async fn evaluate_goal<F, Fut>(
    condition: &str,
    transcript_summary: &str,
    eval_fn: F,
) -> Result<EvalVerdict>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let eval_prompt = build_eval_prompt(condition, transcript_summary);
    let raw_response = eval_fn(eval_prompt).await?;
    Ok(parse_eval_response(&raw_response))
}

fn build_eval_prompt(condition: &str, transcript_summary: &str) -> String {
    format!(
        "You are evaluating whether a goal condition has been met based on the work done so far.\n\n\
         <condition>\n{condition}\n</condition>\n\n\
         <recent_work>\n{transcript_summary}\n</recent_work>\n\n\
         Respond with exactly one JSON object on a single line:\n\
         {{\"status\": \"met|continue|impossible\", \"reason\": \"one sentence\"}}\n\n\
         Rules:\n\
         - \"met\" if the condition is clearly satisfied by the work shown\n\
         - \"impossible\" only if the condition cannot be satisfied in this session\n\
         - \"continue\" if more work is needed\n\
         - Keep the reason concise"
    )
}

pub fn parse_eval_response(raw: &str) -> EvalVerdict {
    let json_str = extract_json_object(raw).unwrap_or(raw);

    #[derive(serde::Deserialize)]
    struct EvalResponse {
        #[serde(default)]
        status: String,
        #[serde(default)]
        reason: String,
    }

    match serde_json::from_str::<EvalResponse>(json_str) {
        Ok(resp) => match resp.status.as_str() {
            "met" => EvalVerdict::Met {
                reasoning: resp.reason,
            },
            "impossible" => EvalVerdict::Impossible {
                reasoning: resp.reason,
            },
            "continue" => EvalVerdict::Continue,
            _ => EvalVerdict::Continue,
        },
        Err(_) => {
            tracing::warn!(raw = %raw, "failed to parse goal eval response, defaulting to continue");
            EvalVerdict::Continue
        }
    }
}

fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}
