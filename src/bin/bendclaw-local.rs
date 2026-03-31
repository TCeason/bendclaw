//! bendclaw-local — local-only agent runtime binary.

use std::sync::Arc;

use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::assembly::local::LocalRuntimeDeps;
use bendclaw::llm::router::LLMRouter;
use bendclaw::local::args::LocalCli;
use bendclaw::local::args::LocalCommand;
use clap::Parser;

#[tokio::main]
async fn main() {
    let agent_config = AgentConfig::default();

    let llm_config = bendclaw::llm::config::LLMConfig::default();
    let llm = Arc::new(LLMRouter::from_config(&llm_config).expect("failed to build LLM provider"));

    let deps = LocalRuntimeDeps::new(agent_config, llm);

    let cli = LocalCli::parse();
    let result = match cli.command {
        LocalCommand::Run(args) => bendclaw::local::cmd_run::execute(args, deps).await,
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
