//! Tests for the ask_user REPL rendering functions.
//!
//! These test the pure `build_*` functions — no terminal IO required.

use bend_engine::tools::AskUserOption;
use bend_engine::tools::AskUserRequest;
use bendclaw::cli::repl::ask_user::build_confirmation;
use bendclaw::cli::repl::ask_user::build_question_block;
use bendclaw::cli::repl::ask_user::build_skipped;

fn two_option_request() -> AskUserRequest {
    AskUserRequest {
        question: "Which cache strategy?".into(),
        options: vec![
            AskUserOption {
                label: "In-memory (Recommended)".into(),
                description: "Zero deps, HashMap + TTL".into(),
            },
            AskUserOption {
                label: "Redis".into(),
                description: "Shared across instances".into(),
            },
        ],
    }
}

fn three_option_request() -> AskUserRequest {
    AskUserRequest {
        question: "Which approach?".into(),
        options: vec![
            AskUserOption {
                label: "Option A (Recommended)".into(),
                description: "First choice".into(),
            },
            AskUserOption {
                label: "Option B".into(),
                description: "Second choice".into(),
            },
            AskUserOption {
                label: "Option C".into(),
                description: "Third choice".into(),
            },
        ],
    }
}

#[test]
fn first_option_highlighted_when_selected_zero() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0);
    // First option should have the › marker
    assert!(output.contains("› 1."));
    // Second option should not
    assert!(output.contains("  2.") || !output.contains("› 2."));
}

#[test]
fn second_option_highlighted_when_selected_one() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 1);
    assert!(output.contains("› 2."));
}

#[test]
fn none_of_above_highlighted_when_selected_last() {
    let req = two_option_request();
    let none_idx = req.options.len(); // index 2
    let (output, _lines) = build_question_block(&req, none_idx);
    assert!(output.contains("› 0. None of the above"));
}

#[test]
fn question_text_appears_in_output() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0);
    assert!(output.contains("Which cache strategy?"));
}

#[test]
fn option_labels_appear_in_output() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0);
    assert!(output.contains("In-memory (Recommended)"));
    assert!(output.contains("Redis"));
}

#[test]
fn option_descriptions_appear_in_output() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0);
    assert!(output.contains("Zero deps, HashMap + TTL"));
    assert!(output.contains("Shared across instances"));
}

#[test]
fn none_of_above_always_present() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0);
    assert!(output.contains("None of the above"));
}

#[test]
fn footer_hint_shows_correct_range() {
    let req = two_option_request();
    let (output, _lines) = build_question_block(&req, 0);
    assert!(output.contains("1-2 pick"));

    let req3 = three_option_request();
    let (output3, _) = build_question_block(&req3, 0);
    assert!(output3.contains("1-3 pick"));
}

#[test]
fn line_count_correct_for_two_options() {
    let req = two_option_request();
    let (_output, lines) = build_question_block(&req, 0);
    // question(1) + blank(1) + opt1_label(1) + opt1_desc(1) + opt2_label(1) + opt2_desc(1)
    // + none_of_above(1) + blank(1) + footer(1) = 9
    assert_eq!(lines, 9);
}

#[test]
fn line_count_correct_for_three_options() {
    let req = three_option_request();
    let (_output, lines) = build_question_block(&req, 0);
    // question(1) + blank(1) + 3*(label+desc)(6) + none(1) + blank(1) + footer(1) = 11
    assert_eq!(lines, 11);
}

#[test]
fn confirmation_contains_checkmark_and_label() {
    let text = build_confirmation("Redis");
    assert!(text.contains("✓"));
    assert!(text.contains("Redis"));
}

#[test]
fn skipped_contains_dash() {
    let text = build_skipped();
    assert!(text.contains("skipped"));
}
