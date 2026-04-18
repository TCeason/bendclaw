use evot::gateway::command::parse_command;
use evot::gateway::command::Command;

#[test]
fn parse_clear() {
    assert!(matches!(parse_command("/clear"), Some(Command::Clear)));
    assert!(matches!(parse_command("/CLEAR"), Some(Command::Clear)));
    assert!(matches!(parse_command("  /clear  "), Some(Command::Clear)));
}

#[test]
fn parse_goto_with_seq() {
    assert!(matches!(parse_command("/goto 5"), Some(Command::Goto(5))));
    assert!(matches!(
        parse_command("/goto 123"),
        Some(Command::Goto(123))
    ));
    assert!(matches!(
        parse_command("  /goto  42  "),
        Some(Command::Goto(42))
    ));
}

#[test]
fn parse_goto_no_arg_returns_usage_error() {
    assert!(matches!(
        parse_command("/goto"),
        Some(Command::UsageError(_))
    ));
    assert!(matches!(
        parse_command("/goto "),
        Some(Command::UsageError(_))
    ));
}

#[test]
fn parse_goto_invalid_arg_returns_usage_error() {
    assert!(matches!(
        parse_command("/goto abc"),
        Some(Command::UsageError(_))
    ));
    assert!(matches!(
        parse_command("/goto -1"),
        Some(Command::UsageError(_))
    ));
    assert!(matches!(
        parse_command("/goto 0"),
        Some(Command::UsageError(_))
    ));
}

#[test]
fn parse_non_command_returns_none() {
    assert!(parse_command("hello").is_none());
    assert!(parse_command("").is_none());
    assert!(parse_command("/unknown").is_none());
    assert!(parse_command("clear").is_none());
}

#[test]
fn parse_history() {
    assert!(matches!(
        parse_command("/history"),
        Some(Command::History(20))
    ));
    assert!(matches!(
        parse_command("/history 10"),
        Some(Command::History(10))
    ));
}

#[test]
fn parse_history_invalid_arg_returns_usage_error() {
    assert!(matches!(
        parse_command("/history abc"),
        Some(Command::UsageError(_))
    ));
    assert!(matches!(
        parse_command("/history 0"),
        Some(Command::UsageError(_))
    ));
}
