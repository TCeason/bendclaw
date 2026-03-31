//! bendclaw-local CLI arguments.

use clap::Parser;
use clap::Subcommand;

#[derive(Parser)]
#[command(name = "bendclaw-local", about = "Local-only agent runtime")]
pub struct LocalCli {
    #[command(subcommand)]
    pub command: LocalCommand,
}

#[derive(Subcommand)]
pub enum LocalCommand {
    /// Run a local agent session
    Run(RunArgs),
}

#[derive(clap::Args)]
pub struct RunArgs {
    /// Prompt text or @file path
    #[arg(long)]
    pub prompt: String,

    /// Resume an existing session
    #[arg(long)]
    pub session_id: Option<String>,

    /// System prompt text or @file path
    #[arg(long)]
    pub system: Option<String>,

    /// Working directory
    #[arg(long)]
    pub cwd: Option<String>,

    /// Tool selection
    #[arg(long, default_value = "coding")]
    pub tools: String,

    /// Maximum turns
    #[arg(long, default_value_t = 50)]
    pub max_turns: u32,

    /// Maximum duration in seconds
    #[arg(long, default_value_t = 600)]
    pub max_duration: u64,
}
