//! Goal evaluator agent wiring.

use std::sync::Arc;

use crate::agent::goal::result_tool::GoalResultCapture;
use crate::agent::goal::result_tool::GoalResultStatus;
use crate::agent::goal::result_tool::GoalResultTool;
use crate::agent::run::runtime::build_agent;
use crate::agent::run::runtime::EngineOptions;
use crate::agent::run::runtime::EvalFn;
use crate::conf::LlmConfig;

pub fn build_eval_fn(llm: &LlmConfig, cwd: &str) -> EvalFn {
    let llm = llm.clone();
    let cwd = cwd.to_string();
    Arc::new(move |prompt: String| {
        let llm = llm.clone();
        let cwd = cwd.clone();
        Box::pin(async move {
            let capture = Arc::new(tokio::sync::Mutex::new(GoalResultCapture::default()));

            let system_prompt = format!(
                "You are evaluating whether a goal condition has been met.\n\n\
                 <evaluation_request>\n{prompt}\n</evaluation_request>\n\n\
                 Use the available tools to inspect the codebase and verify the request.\n\
                 Use as few steps as possible - be efficient and direct.\n\
                 When done, return your result using the goal_result tool with:\n\
                 - ok: true if the condition is met\n\
                 - ok: false with reason if the condition is not met"
            );

            let tools: Vec<Box<dyn evot_engine::AgentTool>> = vec![
                Box::new(evot_engine::tools::ReadFileTool::default()),
                Box::new(evot_engine::tools::ListFilesTool::default()),
                Box::new(evot_engine::tools::SearchTool::default()),
                Box::new(GoalResultTool::new(capture.clone())),
            ];

            let options = EngineOptions {
                provider: llm.provider.clone(),
                protocol: llm.protocol.clone(),
                model: llm.model.clone(),
                api_key: llm.api_key.clone(),
                base_url: Some(llm.base_url.clone()),
                system_prompt,
                limits: crate::agent::ExecutionLimits {
                    max_turns: 50,
                    max_total_tokens: 10_000_000,
                    max_duration_secs: 300,
                },
                skills_dirs: vec![],
                tools,
                thinking_level: evot_engine::ThinkingLevel::Off,
                compat_caps: llm.compat_caps,
                cwd: std::path::PathBuf::from(&cwd),
                path_guard: Arc::new(evot_engine::PathGuard::open()),
                spill_dir: None,
                prompt_cache_key: None,
            };

            let mut engine = build_agent(options, vec![]);
            let user_msg = evot_engine::AgentMessage::Llm(evot_engine::Message::user(
                "Evaluate the goal condition now.",
            ));
            let (handle, mut rx) = engine.submit(vec![user_msg]).await;

            while let Some(event) = rx.recv().await {
                if matches!(event, evot_engine::AgentEvent::ToolExecutionEnd { .. }) {
                    let cap = capture.lock().await;
                    if cap.reason.is_some() {
                        handle.abort();
                        break;
                    }
                }
            }

            let cap = capture.lock().await;
            let text = match cap.status {
                GoalResultStatus::Met => serde_json::json!({
                    "status": "met",
                    "reason": cap.reason.as_deref().unwrap_or("condition met"),
                })
                .to_string(),
                GoalResultStatus::Continue => serde_json::json!({
                    "status": "continue",
                    "reason": cap.reason.as_deref().unwrap_or("evaluator did not return a result"),
                })
                .to_string(),
            };

            Ok(text)
        })
    })
}
