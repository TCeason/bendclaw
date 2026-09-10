//! Wire-order contract for tool parameter schemas.
//!
//! Models tend to emit tool arguments in the order the schema declares them.
//! `path` first means the card headline can name the target file while the body
//! is still streaming, instead of showing an unnamed write until the last
//! argument arrives.
//!
//! This also guards the workspace's `serde_json` `preserve_order` feature:
//! without it `Value` maps are a `BTreeMap`, `json!` sorts keys alphabetically,
//! and `content`/`edits` would serialize ahead of `path`.

use evotengine::tools::EditFileTool;
use evotengine::tools::WriteFileTool;
use evotengine::types::AgentTool;

#[test]
fn write_and_edit_keep_path_ahead_of_the_body() {
    let cases = [
        (WriteFileTool::new().parameters_schema(), "content"),
        (EditFileTool::new().parameters_schema(), "edits"),
    ];
    for (schema, body_key) in cases {
        let object = match schema.as_object() {
            Some(object) => object,
            None => panic!("schema must be a JSON object: {schema}"),
        };

        let properties = match object.get("properties").and_then(|v| v.as_object()) {
            Some(properties) => properties,
            None => panic!("schema must declare properties: {schema}"),
        };
        let property_order: Vec<&str> = properties.keys().map(String::as_str).collect();
        assert_eq!(
            property_order.first(),
            Some(&"path"),
            "path must be the first declared property, got {property_order:?}"
        );

        let required = match object.get("required").and_then(|v| v.as_array()) {
            Some(required) => required,
            None => panic!("schema must declare required: {schema}"),
        };
        let required_order: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(
            required_order,
            vec!["path", body_key],
            "required must list path before the body argument"
        );
    }
}
