use std::path::PathBuf;

/// Initialize tracing once — writes INFO+ logs to ~/.evotai/logs/evot.log.
pub(crate) fn init_tracing() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let log_dir = home.join(".evotai").join("logs");
        let file_appender = tracing_appender::rolling::daily(log_dir, "evot.log");
        let _ = tracing_subscriber::fmt()
            .with_writer(file_appender)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_ansi(false)
            .try_init();
    });
}
