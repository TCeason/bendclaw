//! Tests for line-numbered snippet helpers.

use evotengine::tools::file::snippet::*;

#[test]
fn line_at_counts_newlines_before_offset() {
    assert_eq!(line_at("a\nb\nc", 0), 1);
    assert_eq!(line_at("a\nb\nc", 2), 2);
    assert_eq!(line_at("a\nb\nc", 4), 3);
    assert_eq!(line_at("a\nb\nc", 99), 3);
}

#[test]
fn range_of_span_excludes_trailing_newline_line() {
    // Replacing "b\n" (bytes 2..4) touches only line 2.
    assert_eq!(range_of_span("a\nb\nc\n", 2, 4), LineRange {
        start: 2,
        end: 2
    });
    // Empty span (pure deletion) maps to the line containing the offset.
    assert_eq!(range_of_span("a\nb\nc\n", 2, 2), LineRange {
        start: 2,
        end: 2
    });
}

#[test]
fn widen_clamps_to_file() {
    assert_eq!(LineRange { start: 2, end: 3 }.widen(5, 10), LineRange {
        start: 1,
        end: 8
    });
}

#[test]
fn merge_ranges_joins_adjacent_and_overlapping() {
    let merged = merge_ranges(
        vec![
            LineRange { start: 10, end: 12 },
            LineRange { start: 1, end: 3 },
            LineRange { start: 4, end: 5 },
            LineRange { start: 11, end: 15 },
        ],
        0,
    );
    assert_eq!(merged, vec![LineRange { start: 1, end: 5 }, LineRange {
        start: 10,
        end: 15
    }]);
}

#[test]
fn render_numbers_marks_and_truncates() {
    let text = "a\nb\nc\nd\n";
    let out = render(text, LineRange { start: 1, end: 4 }, &[2], 3);
    assert_eq!(out, "1 | a\n2 ! b\n3 | c\n    ... (1 more lines)\n");
}

#[test]
fn line_count_ignores_final_newline() {
    assert_eq!(line_count("a\nb\n"), 2);
    assert_eq!(line_count("a\nb"), 2);
    assert_eq!(line_count(""), 1);
}
