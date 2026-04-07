use bendclaw::cli::repl::spinner::SpinnerState;

#[test]
fn new_spinner_is_inactive() {
    let state = SpinnerState::new();
    assert!(!state.is_active());
    assert!(state.phase().is_hidden());
}

#[test]
fn activate_sets_verb_phase() {
    let mut state = SpinnerState::new();
    state.activate();
    assert!(state.is_active());
    assert!(state.phase().is_verb());
    assert_eq!(state.frame_index(), 0);
}

#[test]
fn phase_transitions() {
    let mut state = SpinnerState::new();
    state.activate();

    state.set_tool("bash");
    assert!(state.phase().is_tool());

    state.restore_verb();
    assert!(state.phase().is_verb());

    state.deactivate();
    assert!(!state.is_active());
    assert!(state.phase().is_hidden());
}

#[test]
fn glyph_cycles_correctly() {
    let mut state = SpinnerState::new();
    state.activate();

    let count = SpinnerState::glyph_count();
    let first = state.current_glyph().to_string();

    // Advance through all frames
    for _ in 0..count {
        state.render_frame();
    }

    // Should wrap back to the first glyph
    assert_eq!(state.current_glyph(), first);
    assert_eq!(state.frame_index(), count);
}

#[test]
fn add_tokens_accumulates() {
    let mut state = SpinnerState::new();
    state.activate();
    state.add_tokens(100);
    state.add_tokens(50);
    // No public getter for response_tokens, but this should not panic
}

#[test]
fn render_frame_does_nothing_when_inactive() {
    let mut state = SpinnerState::new();
    state.activate();
    state.deactivate();
    state.render_frame();
    // frame should not advance when inactive
    assert_eq!(state.frame_index(), 0);
}

#[test]
fn spinner_stays_active_through_tool_cycle() {
    let mut state = SpinnerState::new();
    state.activate();

    // Simulate ToolStarted -> ToolFinished -> restore_verb
    state.set_tool("bash");
    assert!(state.phase().is_tool());

    state.clear_if_rendered();
    state.restore_verb();
    assert!(state.phase().is_verb());
    assert!(state.is_active());

    // Spinner should still render after tool cycle
    state.render_frame();
    assert_eq!(state.frame_index(), 1);
}

#[test]
fn spinner_renders_continuously_while_active() {
    let mut state = SpinnerState::new();
    state.activate();

    // Render a few frames
    state.render_frame();
    state.render_frame();
    assert_eq!(state.frame_index(), 2);

    // clear_if_rendered does not stop rendering on next tick
    state.clear_if_rendered();
    state.render_frame();
    assert_eq!(state.frame_index(), 3);
}
