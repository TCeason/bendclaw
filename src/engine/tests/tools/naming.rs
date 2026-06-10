//! Tests for `{{tool}}` placeholder resolution in tool-facing text.

use evotengine::tools::resolve_tool_refs;
use evotengine::tools::GrepTool;
use evotengine::tools::SearchTool;
use evotengine::types::AgentTool;

fn tools() -> Vec<Box<dyn AgentTool>> {
    vec![Box::new(SearchTool::new()), Box::new(GrepTool::new())]
}

#[test]
fn resolves_to_claude_alias() {
    let t = tools();
    let out = resolve_tool_refs(
        "use {{grep}} or {{semantic_code_search}}",
        &t,
        "claude-opus-4-6",
    );
    assert_eq!(out, "use Grep or SemanticCodeSearch");
}

#[test]
fn resolves_to_canonical_for_non_claude() {
    let t = tools();
    let out = resolve_tool_refs("use {{grep}} or {{semantic_code_search}}", &t, "gpt-4o");
    assert_eq!(out, "use grep or semantic_code_search");
}

#[test]
fn unknown_placeholder_emits_literal_name() {
    let t = tools();
    let out = resolve_tool_refs("call {{nonexistent_tool}} now", &t, "claude-opus-4-6");
    assert_eq!(out, "call nonexistent_tool now");
}

#[test]
fn text_without_placeholders_is_unchanged() {
    let t = tools();
    let s = "plain text with no braces";
    assert_eq!(resolve_tool_refs(s, &t, "claude-opus-4-6"), s);
}

#[test]
fn unterminated_placeholder_is_emitted_verbatim() {
    let t = tools();
    let out = resolve_tool_refs("dangling {{grep", &t, "claude-opus-4-6");
    assert_eq!(out, "dangling {{grep");
}

#[test]
fn whitespace_inside_placeholder_is_trimmed() {
    let t = tools();
    let out = resolve_tool_refs("use {{ grep }}", &t, "claude-opus-4-6");
    assert_eq!(out, "use Grep");
}
