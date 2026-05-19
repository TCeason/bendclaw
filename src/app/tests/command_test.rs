use evot::gateway::command::parse_command;
use evot::gateway::command::Command;
use evot::gateway::command::GoalCommand;

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

#[test]
fn parse_goal_bare_is_show() {
    assert!(matches!(
        parse_command("/goal"),
        Some(Command::Goal(GoalCommand::Show))
    ));
    assert!(matches!(
        parse_command("  /goal  "),
        Some(Command::Goal(GoalCommand::Show))
    ));
    assert!(matches!(
        parse_command("/goal show"),
        Some(Command::Goal(GoalCommand::Show))
    ));
}

#[test]
fn parse_goal_set_explicit_with_budget() {
    let cmd = parse_command("/goal Refactor the storage layer --budget=20000");
    match cmd {
        Some(Command::Goal(GoalCommand::Set {
            condition,
            max_tokens,
            ..
        })) => {
            assert_eq!(condition, "Refactor the storage layer");
            assert_eq!(max_tokens, Some(20_000));
        }
        other => panic!("expected Goal::Set, got {:?}", debug(&other)),
    }
}

#[test]
fn parse_goal_implicit_set_preserves_case() {
    let cmd = parse_command("/goal Ship the long-running refactor");
    match cmd {
        Some(Command::Goal(GoalCommand::Set {
            condition,
            max_tokens,
            ..
        })) => {
            assert_eq!(condition, "Ship the long-running refactor");
            assert_eq!(max_tokens, None);
        }
        other => panic!("expected Goal::Set, got {:?}", debug(&other)),
    }
}

#[test]
fn parse_goal_pause_resume_done_clear() {
    assert!(matches!(
        parse_command("/goal pause"),
        Some(Command::Goal(GoalCommand::Pause))
    ));
    assert!(matches!(
        parse_command("/goal resume"),
        Some(Command::Goal(GoalCommand::Resume))
    ));
    assert!(matches!(
        parse_command("/goal done"),
        Some(Command::Goal(GoalCommand::Done { reason: None }))
    ));
    assert!(matches!(
        parse_command("/goal done tests passed"),
        Some(Command::Goal(GoalCommand::Done { reason: Some(reason) })) if reason == "tests passed"
    ));
    assert!(matches!(
        parse_command("/goal clear"),
        Some(Command::Goal(GoalCommand::Clear))
    ));
}

#[test]
fn parse_goal_invalid_budget_is_usage_error() {
    assert!(matches!(
        parse_command("/goal set foo --budget=0"),
        Some(Command::UsageError(_))
    ));
    assert!(matches!(
        parse_command("/goal set foo --budget=abc"),
        Some(Command::UsageError(_))
    ));
}

#[test]
fn parse_goal_set_without_objective_is_usage_error() {
    // Only flags, no condition text → usage error.
    assert!(matches!(
        parse_command("/goal --budget=100"),
        Some(Command::UsageError(_))
    ));
}

#[test]
fn parse_dump_bare_has_no_target() {
    match parse_command("/_dump") {
        Some(Command::Dump { target: None }) => {}
        other => panic!("expected Dump{{ target: None }}, got {:?}", debug(&other)),
    }
}

#[test]
fn parse_dump_with_path() {
    match parse_command("/_dump /tmp/p.json") {
        Some(Command::Dump { target: Some(t) }) => assert_eq!(t, "/tmp/p.json"),
        other => panic!("expected Dump with path, got {:?}", debug(&other)),
    }
}

fn debug(cmd: &Option<Command>) -> &'static str {
    match cmd {
        None => "None",
        Some(Command::Clear) => "Clear",
        Some(Command::Goto(_)) => "Goto",
        Some(Command::History(_)) => "History",
        Some(Command::Goal(GoalCommand::Show)) => "Goal::Show",
        Some(Command::Goal(GoalCommand::Set { .. })) => "Goal::Set",
        Some(Command::Goal(GoalCommand::Pause)) => "Goal::Pause",
        Some(Command::Goal(GoalCommand::Resume)) => "Goal::Resume",
        Some(Command::Goal(GoalCommand::Done { .. })) => "Goal::Done",
        Some(Command::Goal(GoalCommand::Clear)) => "Goal::Clear",
        Some(Command::Dump { .. }) => "Dump",
        Some(Command::UsageError(_)) => "UsageError",
    }
}
