//! Tests for FileReadState (context/file_state.rs).

use evotengine::context::file_state::FileReadState;

#[test]
fn record_and_check_unchanged() {
    let mut state = FileReadState::new();
    state.record("/workspace/foo.rs", 1000, 42);

    assert!(state.is_unchanged("/workspace/foo.rs", 1000).is_some());
    assert!(state.is_unchanged("/workspace/foo.rs", 2000).is_none());
    assert!(state.is_unchanged("/workspace/bar.rs", 1000).is_none());
}

#[test]
fn invalidate_removes_entry() {
    let mut state = FileReadState::new();
    state.record("/workspace/foo.rs", 1000, 42);
    state.invalidate("/workspace/foo.rs");

    assert!(state.is_unchanged("/workspace/foo.rs", 1000).is_none());
}

#[test]
fn recent_files_sorted_by_recency() {
    let mut state = FileReadState::new();
    state.record("/a.rs", 100, 10);
    state.set_read_at("/a.rs", 1);

    state.record("/b.rs", 200, 20);
    state.set_read_at("/b.rs", 3);

    state.record("/c.rs", 300, 30);
    state.set_read_at("/c.rs", 2);

    let recent = state.recent_files(2);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].path, "/b.rs");
    assert_eq!(recent[1].path, "/c.rs");
}

#[test]
fn recent_files_respects_max() {
    let mut state = FileReadState::new();
    state.record("/a.rs", 100, 10);
    state.record("/b.rs", 200, 20);
    state.record("/c.rs", 300, 30);

    let recent = state.recent_files(1);
    assert_eq!(recent.len(), 1);
}

#[test]
fn record_updates_existing_entry() {
    let mut state = FileReadState::new();
    state.record("/foo.rs", 1000, 42);
    state.record("/foo.rs", 2000, 50);

    assert!(state.is_unchanged("/foo.rs", 1000).is_none());
    assert!(state.is_unchanged("/foo.rs", 2000).is_some());
    let entry = state.is_unchanged("/foo.rs", 2000).unwrap();
    assert_eq!(entry.total_lines, 50);
}
