use bendclaw::cli::repl::commands::is_slash_command;
use bendclaw::cli::repl::commands::resolve_slash_command;
use bendclaw::cli::repl::commands::ResolvedSlashCommand;
use bendclaw::cli::repl::commands::KNOWN_COMMANDS;
use bendclaw::cli::repl::completion::bare_slash_hint_display;
use bendclaw::cli::repl::completion::is_slash_prefix;

// ---------------------------------------------------------------------------
// is_slash_prefix
// ---------------------------------------------------------------------------

#[test]
fn slash_prefix_bare_slash() {
    assert!(is_slash_prefix("/"));
}

#[test]
fn slash_prefix_valid_commands() {
    assert!(is_slash_prefix("/help"));
    assert!(is_slash_prefix("/status"));
    assert!(is_slash_prefix("/clear"));
    assert!(is_slash_prefix("/quit"));
}

#[test]
fn slash_prefix_partial_command() {
    assert!(is_slash_prefix("/he"));
    assert!(is_slash_prefix("/s"));
    assert!(is_slash_prefix("/cl"));
}

#[test]
fn slash_prefix_with_bang() {
    assert!(is_slash_prefix("/clear!"));
}

#[test]
fn slash_prefix_with_arg() {
    assert!(is_slash_prefix("/help model"));
    assert!(is_slash_prefix("/resume abc123"));
}

#[test]
fn slash_prefix_rejects_file_paths() {
    assert!(!is_slash_prefix("/some/path.rs"));
    assert!(!is_slash_prefix("/usr/local/bin"));
    assert!(!is_slash_prefix("/foo/bar"));
}

#[test]
fn slash_prefix_rejects_paths_with_dots() {
    assert!(!is_slash_prefix("/file.rs"));
    assert!(!is_slash_prefix("/a.b"));
}

#[test]
fn slash_prefix_rejects_paths_with_digits() {
    assert!(!is_slash_prefix("/123"));
    assert!(!is_slash_prefix("/file2"));
}

#[test]
fn slash_prefix_rejects_uppercase() {
    assert!(!is_slash_prefix("/Help"));
    assert!(!is_slash_prefix("/STATUS"));
}

#[test]
fn slash_prefix_rejects_no_slash() {
    assert!(!is_slash_prefix("help"));
    assert!(!is_slash_prefix(""));
    assert!(!is_slash_prefix(":/foo/bar"));
}

// ---------------------------------------------------------------------------
// is_slash_command
// ---------------------------------------------------------------------------

#[test]
fn slash_command_known_commands() {
    for cmd in KNOWN_COMMANDS {
        assert!(is_slash_command(cmd), "expected {cmd} to be recognized");
    }
}

#[test]
fn slash_command_with_args() {
    assert!(is_slash_command("/help model"));
    assert!(is_slash_command("/resume abc123"));
    assert!(is_slash_command("/model claude-3"));
}

#[test]
fn slash_command_rejects_unknown() {
    assert!(!is_slash_command("/unknown"));
    assert!(!is_slash_command("/foo"));
}

#[test]
fn slash_command_rejects_pasted_paths() {
    assert!(!is_slash_command("/some/path.rs"));
    assert!(!is_slash_command("/usr/local/bin"));
    assert!(!is_slash_command("look at :/foo/bar.rs"));
}

#[test]
fn slash_command_rejects_empty() {
    assert!(!is_slash_command(""));
    assert!(!is_slash_command("/"));
    assert!(!is_slash_command("  "));
}

// ---------------------------------------------------------------------------
// bare_slash_hint_display
// ---------------------------------------------------------------------------

#[test]
fn bare_slash_hint_contains_all_commands() {
    let display = bare_slash_hint_display();
    for cmd in KNOWN_COMMANDS {
        let name = &cmd[1..];
        assert!(
            display.contains(name),
            "bare slash hint should contain '{name}', got: {display}"
        );
    }
}

#[test]
fn bare_slash_hint_is_bracketed() {
    let display = bare_slash_hint_display();
    assert!(display.contains('['), "hint should start with '['");
    assert!(display.contains(']'), "hint should end with ']'");
}

// ---------------------------------------------------------------------------
// resolve_slash_command
// ---------------------------------------------------------------------------

#[test]
fn resolve_exact_commands() {
    for cmd in KNOWN_COMMANDS {
        assert_eq!(
            resolve_slash_command(cmd),
            ResolvedSlashCommand::Resolved(cmd.to_string()),
            "exact command {cmd} should resolve to itself"
        );
    }
}

#[test]
fn resolve_exact_with_args() {
    assert_eq!(
        resolve_slash_command("/help model"),
        ResolvedSlashCommand::Resolved("/help model".into())
    );
    assert_eq!(
        resolve_slash_command("/resume abc123"),
        ResolvedSlashCommand::Resolved("/resume abc123".into())
    );
    assert_eq!(
        resolve_slash_command("/model claude-3"),
        ResolvedSlashCommand::Resolved("/model claude-3".into())
    );
    assert_eq!(
        resolve_slash_command("/env set KEY=VAL"),
        ResolvedSlashCommand::Resolved("/env set KEY=VAL".into())
    );
}

#[test]
fn resolve_unique_prefix_single_letter() {
    assert_eq!(
        resolve_slash_command("/h"),
        ResolvedSlashCommand::Resolved("/help".into())
    );
    assert_eq!(
        resolve_slash_command("/r"),
        ResolvedSlashCommand::Resolved("/resume".into())
    );
    assert_eq!(
        resolve_slash_command("/n"),
        ResolvedSlashCommand::Resolved("/new".into())
    );
    assert_eq!(
        resolve_slash_command("/m"),
        ResolvedSlashCommand::Resolved("/model".into())
    );
    assert_eq!(
        resolve_slash_command("/p"),
        ResolvedSlashCommand::Resolved("/plan".into())
    );
    assert_eq!(
        resolve_slash_command("/a"),
        ResolvedSlashCommand::Resolved("/act".into())
    );
    assert_eq!(
        resolve_slash_command("/e"),
        ResolvedSlashCommand::Resolved("/env".into())
    );
    assert_eq!(
        resolve_slash_command("/l"),
        ResolvedSlashCommand::Resolved("/log".into())
    );
}

#[test]
fn resolve_unique_prefix_multiple_letters() {
    assert_eq!(
        resolve_slash_command("/he"),
        ResolvedSlashCommand::Resolved("/help".into())
    );
    assert_eq!(
        resolve_slash_command("/hel"),
        ResolvedSlashCommand::Resolved("/help".into())
    );
    assert_eq!(
        resolve_slash_command("/res"),
        ResolvedSlashCommand::Resolved("/resume".into())
    );
    assert_eq!(
        resolve_slash_command("/mo"),
        ResolvedSlashCommand::Resolved("/model".into())
    );
}

#[test]
fn resolve_prefix_with_args() {
    assert_eq!(
        resolve_slash_command("/h model"),
        ResolvedSlashCommand::Resolved("/help model".into())
    );
    assert_eq!(
        resolve_slash_command("/r abc123"),
        ResolvedSlashCommand::Resolved("/resume abc123".into())
    );
    assert_eq!(
        resolve_slash_command("/m claude-3"),
        ResolvedSlashCommand::Resolved("/model claude-3".into())
    );
    assert_eq!(
        resolve_slash_command("/e set KEY=VAL"),
        ResolvedSlashCommand::Resolved("/env set KEY=VAL".into())
    );
    assert_eq!(
        resolve_slash_command("/l what happened"),
        ResolvedSlashCommand::Resolved("/log what happened".into())
    );
}

#[test]
fn resolve_unknown_slash_command() {
    assert_eq!(resolve_slash_command("/x"), ResolvedSlashCommand::Unknown);
    assert_eq!(
        resolve_slash_command("/hello"),
        ResolvedSlashCommand::Unknown
    );
    assert_eq!(
        resolve_slash_command("/foo bar"),
        ResolvedSlashCommand::Unknown
    );
}

#[test]
fn resolve_rejects_paths() {
    assert_eq!(
        resolve_slash_command("/tmp/a.txt"),
        ResolvedSlashCommand::Unknown
    );
    assert_eq!(
        resolve_slash_command("/usr/local/bin"),
        ResolvedSlashCommand::Unknown
    );
    assert_eq!(
        resolve_slash_command("/foo/bar"),
        ResolvedSlashCommand::Unknown
    );
    assert_eq!(
        resolve_slash_command("/file.rs"),
        ResolvedSlashCommand::Unknown
    );
}

#[test]
fn resolve_rejects_non_slash_input() {
    assert_eq!(resolve_slash_command("help"), ResolvedSlashCommand::Unknown);
    assert_eq!(resolve_slash_command(""), ResolvedSlashCommand::Unknown);
    assert_eq!(resolve_slash_command("  "), ResolvedSlashCommand::Unknown);
}

#[test]
fn resolve_bare_slash() {
    assert_eq!(resolve_slash_command("/"), ResolvedSlashCommand::Unknown);
}
