//! Tests for the not-found closest-match hint.

use evotengine::tools::file::hint::*;
use evotengine::tools::file::snippet::LineRange;

const CONTENT: &str = "fn a() {}\nfn b() {\n    call(1);\n    call(2);\n}\nfn c() {}\n";

#[test]
fn closest_picks_window_with_most_matching_lines() {
    let c = closest(CONTENT, "fn b() {\n    call(1);\n    call(3);\n}").unwrap();
    assert_eq!(c.range, LineRange { start: 2, end: 5 });
    assert_eq!(c.differing, vec![4]);
    assert_eq!(c.matched, 3);
}

#[test]
fn closest_ignores_indentation_differences() {
    let c = closest(CONTENT, "call(1);\ncall(2);").unwrap();
    assert_eq!(c.range, LineRange { start: 3, end: 4 });
    assert!(c.differing.is_empty());
}

#[test]
fn closest_falls_back_to_distinctive_line_anchor() {
    // Only 1 of 3 lines matches (< half), but "    call(2);" is a distinctive anchor.
    let c = closest(CONTENT, "zzz\nyyy\n    call(2);").unwrap();
    assert_eq!(c.range, LineRange { start: 2, end: 4 });
}

#[test]
fn closest_none_when_nothing_similar() {
    assert!(closest(CONTENT, "nothing\nhere").is_none());
    assert!(closest(CONTENT, "").is_none());
}

#[test]
fn hint_renders_marked_snippet() {
    let h = not_found_hint(CONTENT, "fn b() {\n    call(1);\n    call(3);\n}").unwrap();
    assert!(
        h.starts_with("Closest match at lines 2-5 (3 of 4 lines identical"),
        "{h}"
    );
    assert!(h.contains("4 !     call(2);\n"), "{h}");
    assert!(h.contains("1 | fn a() {}\n"), "{h}");
}
