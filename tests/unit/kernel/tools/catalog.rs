//! Tests for tools/catalog — verifies each catalog layer registers expected tools.

use std::sync::Arc;

use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::services::NoopSecretUsageSink;

#[test]
fn core_catalog_registers_file_and_shell_tools() {
    let mut registry = ToolRegistry::new();
    let sink: Arc<dyn bendclaw::kernel::tools::services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    bendclaw::kernel::tools::catalog::core::register(&mut registry, sink);

    let schemas = registry.tool_schemas();
    let names: Vec<String> = schemas.iter().map(|t| t.function.name.clone()).collect();

    // Core tools should include file, search, shell, web
    assert!(
        names.iter().any(|n| n.contains("file_read")),
        "missing file_read: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("shell")),
        "missing shell: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("grep")),
        "missing grep: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("glob")),
        "missing glob: {names:?}"
    );
    assert!(
        !names.is_empty(),
        "core catalog should register at least some tools"
    );
}

#[test]
fn core_catalog_tool_schemas_have_descriptions() {
    let mut registry = ToolRegistry::new();
    let sink: Arc<dyn bendclaw::kernel::tools::services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    bendclaw::kernel::tools::catalog::core::register(&mut registry, sink);

    for schema in registry.tool_schemas() {
        assert!(
            !schema.function.description.is_empty(),
            "tool '{}' has empty description",
            schema.function.name
        );
    }
}
