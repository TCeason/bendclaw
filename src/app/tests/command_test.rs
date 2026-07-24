use evot::gateway::command::parse_command;
use evot::gateway::command::Command;

#[test]
fn parse_clear() {
    assert!(matches!(parse_command("/clear"), Some(Command::Clear)));
    assert!(matches!(parse_command("/CLEAR"), Some(Command::Clear)));
    assert!(matches!(parse_command("  /clear  "), Some(Command::Clear)));
}

#[test]
fn parse_compact_with_optional_instructions() {
    assert!(matches!(
        parse_command("/compact"),
        Some(Command::Compact {
            custom_instructions: None
        })
    ));
    assert!(matches!(
        parse_command("/COMPACT preserve implementation details"),
        Some(Command::Compact { custom_instructions: Some(ref value) })
            if value == "preserve implementation details"
    ));
}

#[test]
fn parse_clip_all_is_clip_session() {
    assert!(matches!(
        parse_command("/clip all"),
        Some(Command::ClipSession)
    ));
    assert!(matches!(
        parse_command("  /clip all  "),
        Some(Command::ClipSession)
    ));
    assert!(matches!(
        parse_command("/CLIP ALL"),
        Some(Command::ClipSession)
    ));
}

#[test]
fn parse_bare_or_invalid_clip_is_usage_error() {
    assert!(matches!(
        parse_command("/clip"),
        Some(Command::UsageError(_))
    ));
    assert!(matches!(
        parse_command("/clip custom-name"),
        Some(Command::UsageError(_))
    ));
}

#[test]
fn clip_session_prompt_mentions_skill() {
    use evot::gateway::command::clip_session_prompt;
    let prompt = clip_session_prompt();
    assert!(prompt.contains("memory"));
    assert!(prompt.contains("archive"));
}

#[test]
fn parse_rsearch() {
    assert!(matches!(
        parse_command("/_rsearch tailscale migration"),
        Some(Command::ResumeSearch { ref query }) if query == "tailscale migration"
    ));
    assert!(matches!(
        parse_command("/_rsearch"),
        Some(Command::UsageError(_))
    ));
}

#[test]
fn parse_non_command_returns_none() {
    assert!(parse_command("hello").is_none());
    assert!(parse_command("").is_none());
    assert!(parse_command("/unknown").is_none());
    assert!(parse_command("/goto").is_none());
    assert!(parse_command("/goto 10").is_none());
    assert!(parse_command("/history").is_none());
    assert!(parse_command("/history 10").is_none());
    assert!(parse_command("clear").is_none());
}
