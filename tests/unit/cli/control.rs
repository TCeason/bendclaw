use bendclaw::cli::control::default_log_dir;
use bendclaw::cli::control::dirs_home;
use bendclaw::config::EVOTAI_DIR_NAME;

#[test]
fn default_background_log_dir_uses_evotai_logs() {
    assert_eq!(
        default_log_dir(),
        dirs_home().join(EVOTAI_DIR_NAME).join("logs")
    );
}
