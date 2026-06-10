//! Model-aware tool-name substitution for tool-facing text.
//!
//! Tool descriptions and guidelines must refer to other tools by the name the
//! target model actually sees. On Claude the explore tools are presented as
//! `Grep`/`Glob`/`Read`/`SemanticCodeSearch`; elsewhere they keep their
//! canonical snake_case names. Hardcoding either form into a description makes
//! it desync from the callable name for half the models.
//!
//! Authors write a `{{canonical_name}}` placeholder instead, and this resolver
//! rewrites it to `tool.resolve_name(model)` at prompt-/schema-build time.
//! Unknown names are emitted verbatim (minus the braces) so a typo degrades to
//! plain text rather than leaking `{{...}}` to the model.

use crate::types::AgentTool;

/// Replace `{{canonical_name}}` placeholders in `text` with the name each tool
/// is presented under to `model`. Names with no matching tool are emitted as
/// their literal canonical form.
pub fn resolve_tool_refs(text: &str, tools: &[Box<dyn AgentTool>], model: &str) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = after[..end].trim();
                let resolved = tools
                    .iter()
                    .find(|t| t.name() == key)
                    .map(|t| t.resolve_name(model))
                    .unwrap_or_else(|| key.to_string());
                out.push_str(&resolved);
                rest = &after[end + 2..];
            }
            None => {
                // Unterminated placeholder — emit the rest verbatim.
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
