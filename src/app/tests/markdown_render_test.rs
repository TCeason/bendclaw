use bendclaw::cli::repl::markdown::render::Renderer;
use streamdown_parser::inline::InlineElement;
use streamdown_parser::ParseEvent;

fn render_events(width: usize, events: &[ParseEvent]) -> String {
    let mut out = Vec::new();
    {
        let mut renderer = Renderer::new(&mut out, width);
        for event in events {
            renderer.render_event(event).unwrap_or_else(|err| {
                panic!("render_event failed: {err}");
            });
        }
    }
    String::from_utf8(out).unwrap_or_else(|err| panic!("utf8 conversion failed: {err}"))
}

#[test]
fn table_uses_box_drawing_borders() {
    let output = render_events(80, &[
        ParseEvent::TableHeader(vec!["Name".into(), "Value".into()]),
        ParseEvent::TableSeparator,
        ParseEvent::TableRow(vec!["foo".into(), "bar".into()]),
        ParseEvent::TableEnd,
    ]);

    assert!(output.contains("┌"));
    assert!(output.contains("┬"));
    assert!(output.contains("┐"));
    assert!(output.contains("├"));
    assert!(output.contains("┼"));
    assert!(output.contains("┤"));
    assert!(output.contains("└"));
    assert!(output.contains("┴"));
    assert!(output.contains("┘"));
}

#[test]
fn table_handles_wide_unicode_content() {
    let output = render_events(80, &[
        ParseEvent::TableHeader(vec!["列".into(), "值".into()]),
        ParseEvent::TableSeparator,
        ParseEvent::TableRow(vec!["中文".into(), "emoji 😀".into()]),
        ParseEvent::TableEnd,
    ]);

    assert!(output.contains("中文"));
    assert!(output.contains("emoji 😀"));
    assert!(output.contains("┌"));
}

#[test]
fn narrow_table_falls_back_to_vertical_format() {
    let output = render_events(20, &[
        ParseEvent::TableHeader(vec!["Column A".into(), "Column B".into()]),
        ParseEvent::TableSeparator,
        ParseEvent::TableRow(vec![
            "A very long value that should wrap vertically".into(),
            "Another long value".into(),
        ]),
        ParseEvent::TableEnd,
    ]);

    assert!(output.contains("Column A:"));
    assert!(output.contains("Column B:"));
    assert!(!output.contains("┌"));
    assert!(!output.contains("┬"));
}

#[test]
fn issue_references_are_linkified() {
    let output = render_events(80, &[ParseEvent::Text("see evotai/bendclaw#123".into())]);

    assert!(output.contains("https://github.com/evotai/bendclaw/issues/123"));
    assert!(output.contains("evotai/bendclaw#123"));
    assert!(output.contains("\x1b]8;;"));
}

#[test]
fn inline_text_issue_references_are_linkified() {
    let output = render_events(80, &[ParseEvent::InlineElements(vec![
        InlineElement::Text("refs evotai/bendclaw#456".into()),
    ])]);

    assert!(output.contains("https://github.com/evotai/bendclaw/issues/456"));
    assert!(output.contains("evotai/bendclaw#456"));
}

#[test]
fn url_fragments_are_not_treated_as_issue_refs() {
    let output = render_events(80, &[ParseEvent::Text(
        "docs: https://example.com/page#section".into(),
    )]);

    assert!(!output.contains("github.com"));
    assert!(output.contains("page#section"));
}
