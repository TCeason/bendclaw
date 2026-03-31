use std::sync::Arc;

use clap::Args;

use crate::cli::input;
use crate::cli::output;
use crate::config::BendClawConfig;
use crate::kernel::invocation::request::*;
use crate::kernel::run::result::Reason;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::options::RunOptions;
use crate::kernel::tools::selection;
use crate::llm::router::LLMRouter;

#[derive(Args)]
pub struct AgentArgs {
    /// Prompt text or @file reference
    #[arg(short, long)]
    pub prompt: String,

    /// Agent ID — when set, uses cloud config
    #[arg(long)]
    pub agent_id: Option<String>,

    /// User ID — required when --agent-id is set
    #[arg(long)]
    pub user_id: Option<String>,

    /// Skill file path (injected as skill overlay)
    #[arg(short, long)]
    pub skill: Option<String>,

    /// System prompt override (string or @file)
    #[arg(long)]
    pub system: Option<String>,

    /// Working directory for the agent
    #[arg(long)]
    pub cwd: Option<String>,

    /// Tool filter: "all", "coding", "file,shell", etc.
    #[arg(long, default_value = "coding")]
    pub tools: String,

    /// Maximum agent loop iterations
    #[arg(long, default_value = "50")]
    pub max_turns: u32,

    /// Maximum duration in seconds
    #[arg(long, default_value = "600")]
    pub max_duration: u64,
}

pub async fn cmd_agent(config: BendClawConfig, args: AgentArgs) -> anyhow::Result<()> {
    let prompt = input::resolve_at_file(&args.prompt)?;
    let system_overlay = args
        .system
        .map(|s| input::resolve_at_file(&s))
        .transpose()?;
    let skill_content = args
        .skill
        .map(|p| std::fs::read_to_string(&p))
        .transpose()
        .map_err(|e| anyhow::anyhow!("failed to read skill file: {e}"))?;
    let tool_filter = selection::parse_tool_selection(&args.tools);

    let source = match args.agent_id {
        Some(ref aid) => {
            let uid = args
                .user_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--user-id is required when --agent-id is set"))?;
            ConfigSource::Cloud {
                agent_id: aid.clone(),
                user_id: uid.to_string(),
            }
        }
        None => ConfigSource::Local,
    };

    let llm = Arc::new(LLMRouter::from_config(&config.llm)?);

    let runtime = Runtime::new(
        &config.storage.databend_api_base_url,
        &config.storage.databend_api_token,
        &config.storage.databend_warehouse,
        &config.storage.db_prefix,
        &config.node_id,
        llm,
    )
    .with_workspace(config.workspace.clone())
    .build_minimal()
    .await?;

    let output_result = runtime
        .run_once_invocation(InvocationRequest {
            source,
            persistence: PersistenceMode::Noop,
            context: ConversationContext::None,
            prompt,
            options: RunOptions {
                system_overlay,
                skill_overlay: skill_content,
                max_iterations: Some(args.max_turns),
                max_duration_secs: Some(args.max_duration),
            },
            session_options: SessionBuildOptions {
                cwd: args.cwd.map(std::path::PathBuf::from),
                tool_filter,
                llm_override: None,
            },
        })
        .await?;

    output::print_run_output(&output_result);

    let exit_code = if output_result.stop_reason == Reason::EndTurn {
        0
    } else {
        1
    };
    std::process::exit(exit_code);
}
