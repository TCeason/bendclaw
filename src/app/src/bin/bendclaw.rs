use bend_base::logx;
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let env_filter = match tracing_subscriber::EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) => "info".into(),
    };

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    logx!(
        info,
        "app",
        "started",
        msg = "build info",
        git_sha = env!("BENDCLAW_GIT_SHA"),
        git_branch = env!("BENDCLAW_GIT_BRANCH"),
        build_timestamp = env!("BENDCLAW_BUILD_TIMESTAMP"),
    );

    let args = bendclaw::cli::CliArgs::parse();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        bendclaw::cli::run_cli(args)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
}
