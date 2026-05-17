//! Goal verifier agent wiring.

use std::sync::Arc;

use crate::agent::goal::result_tool::GoalResultCapture;
use crate::agent::goal::result_tool::GoalResultTool;
use crate::agent::goal::GoalVerdict;
use crate::agent::run::runtime::build_agent;
use crate::agent::run::runtime::EngineOptions;
use crate::agent::run::runtime::VerifyFn;
use crate::conf::LlmConfig;
use crate::error::EvotError;

pub fn build_verify_fn(llm: &LlmConfig, cwd: &str) -> VerifyFn {
    let llm = llm.clone();
    let cwd = cwd.to_string();
    Arc::new(move |prompt: String| {
        let llm = llm.clone();
        let cwd = cwd.clone();
        Box::pin(async move {
            let capture = Arc::new(tokio::sync::Mutex::new(GoalResultCapture::default()));

            let system_prompt = format!(
                "You are a goal stop verifier.\n\n\
                 <verification_request>\n{prompt}\n</verification_request>\n\n\
                 Be strict and concise. Verify completion, do not continue the task yourself.\n\
                 Use read-only tools only when the transcript is insufficient.\n\
                 Finish by calling goal_result exactly once."
            );

            let tools: Vec<Box<dyn evot_engine::AgentTool>> = vec![
                Box::new(evot_engine::tools::ReadFileTool::default()),
                Box::new(evot_engine::tools::ListFilesTool::default()),
                Box::new(evot_engine::tools::SearchTool::default()),
                Box::new(GoalResultTool::new(capture.clone())),
            ];

            let options = EngineOptions {
                provider: llm.provider.clone(),
                protocol: llm.protocol,
                model: llm.model.clone(),
                api_key: llm.api_key.clone(),
                base_url: Some(llm.base_url.clone()),
                system_prompt,
                limits: crate::agent::ExecutionLimits {
                    max_turns: 6,
                    max_total_tokens: 20_000,
                    max_duration_secs: 120,
                },
                skills_dirs: Vec::new(),
                tools,
                thinking_level: evot_engine::ThinkingLevel::Off,
                compat_caps: llm.compat_caps,
                cwd: std::path::PathBuf::from(&cwd),
                path_guard: Arc::new(evot_engine::PathGuard::open()),
                spill_dir: None,
                prompt_cache_key: None,
            };

            let mut engine = build_agent(options, vec![]);
            let user_msg = evot_engine::AgentMessage::Llm(evot_engine::Message::User {
                content: vec![evot_engine::Content::Text {
                    text: "Verify whether the goal should stop now.".to_string(),
                }],
                timestamp: evot_engine::now_ms(),
            });
            let (handle, mut rx) = engine.submit(vec![user_msg]).await;

            while let Some(event) = rx.recv().await {
                if matches!(event, evot_engine::AgentEvent::ToolExecutionEnd { .. }) {
                    let cap = capture.lock().await;
                    if cap.ok.is_some() {
                        handle.abort();
                        break;
                    }
                }
            }

            let cap = capture.lock().await;
            let reason = if cap.reason.trim().is_empty() {
                "verifier did not provide a reason".to_string()
            } else {
                cap.reason.clone()
            };

            match cap.ok {
                Some(true) => Ok(GoalVerdict::Met { reason }),
                Some(false) => Ok(GoalVerdict::NotMet { reason }),
                None => Err(EvotError::Agent(
                    "goal verifier did not return a structured result".into(),
                )),
            }
        })
    })
}
