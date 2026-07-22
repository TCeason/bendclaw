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
fn parse_mem_bare_is_memorize() {
    assert!(matches!(parse_command("/mem"), Some(Command::Memorize)));
    assert!(matches!(parse_command("  /mem  "), Some(Command::Memorize)));
    assert!(matches!(parse_command("/MEM"), Some(Command::Memorize)));
}

#[test]
fn parse_mem_with_terms_is_search() {
    assert!(matches!(
        parse_command("/mem aa bb"),
        Some(Command::MemorySearch { ref query }) if query == "aa bb"
    ));
}

#[test]
fn memorize_prompt_mentions_skill() {
    use evot::gateway::command::memorize_prompt;
    let prompt = memorize_prompt();
    assert!(prompt.contains("memory"));
    assert!(prompt.contains("archive"));
}

#[test]
fn recall_prompt_embeds_query_and_asks_for_paths() {
    use evot::gateway::command::recall_prompt;
    let prompt = recall_prompt("tailscale account");
    assert!(prompt.contains("memory"));
    assert!(prompt.contains("tailscale account"));
    assert!(prompt.contains(".md file path"));
    assert!(prompt.contains("synonyms"));
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
