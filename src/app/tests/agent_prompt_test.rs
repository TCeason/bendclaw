use evot::agent::prompt::dynamic_sections;
use evot::agent::prompt::DynamicContext;
use evot::agent::prompt::PromptMode;
use evot::agent::prompt::Section;
use evot::agent::prompt::SystemPrompt;

/// Default coding tool set (read, bash, edit, write), mirroring production.
fn coding_tools() -> Vec<Box<dyn evot_engine::AgentTool>> {
    use evot_engine::tools::BashTool;
    use evot_engine::tools::EditFileTool;
    use evot_engine::tools::ReadFileTool;
    use evot_engine::tools::WriteFileTool;
    vec![
        Box::new(ReadFileTool::default()),
        Box::new(BashTool::default()),
        Box::new(EditFileTool::new()),
        Box::new(WriteFileTool::new()),
    ]
}

fn base_prompt(cwd: &str) -> String {
    SystemPrompt::base(cwd, &coding_tools(), "").0
}

fn names(sections: &[Section]) -> Vec<&'static str> {
    sections.iter().map(|s| s.name).collect()
}

#[test]
fn base_prompt_contains_section_headers() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = base_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("Using your tools:"));
    assert!(prompt.contains("Current working directory:"));
    assert!(prompt.contains("Current date:"));
    assert!(!prompt.contains("Project Instructions"));
}

#[test]
fn reads_single_context_file() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("EVOT.md"), "# My Project\nDo X.")
        .expect("failed to write file");
    let prompt = base_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("Project Instructions"));
    assert!(prompt.contains("My Project"));
}

#[test]
fn concatenates_multiple_context_files() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("EVOT.md"), "part one").expect("failed to write file");
    std::fs::write(tmp.path().join("CLAUDE.md"), "part two").expect("failed to write file");
    let prompt = base_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("part one"));
    assert!(prompt.contains("part two"));
}

#[test]
fn skips_empty_context_files() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("EVOT.md"), "   ").expect("failed to write file");
    let prompt = base_prompt(&tmp.path().to_string_lossy());
    assert!(!prompt.contains("Project Instructions"));
}

#[test]
fn base_sections_are_ordered_static_then_dynamic_boundary() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = base_prompt(&tmp.path().to_string_lossy());
    let guidelines_pos = prompt
        .find("Using your tools:")
        .expect("missing Using your tools:");
    let cwd_pos = prompt
        .find("Current working directory:")
        .expect("missing Current working directory:");
    let boundary_pos = prompt
        .find("__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__")
        .expect("missing dynamic boundary");
    let date_pos = prompt.find("Current date:").expect("missing date");

    assert!(guidelines_pos < cwd_pos, "guidelines should precede cwd");
    assert!(cwd_pos < boundary_pos, "cwd should precede boundary");
    assert!(boundary_pos < date_pos, "boundary should precede date");
}

#[test]
fn tool_set_drives_identity_list_and_guidelines() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = base_prompt(&tmp.path().to_string_lossy());

    // Identity "Available tools" list is derived from each tool's snippet.
    assert!(prompt.contains("Available tools:"));
    assert!(prompt.contains("- read: Read file contents"));
    assert!(prompt.contains("- write: Create or overwrite files"));

    // With the pi-aligned default coding set (read/bash/edit/write and no
    // dedicated search tools), file exploration is steered through bash rather
    // than discouraged. Mirrors pi's "Use bash for file operations" guideline.
    assert!(prompt.contains("Use bash for file operations like ls, rg, find"));
    // The anti-bash framing only applies when dedicated search tools exist, so
    // it must NOT appear here.
    assert!(!prompt.contains("Do not run a bash command when a dedicated tool exists"));
    assert!(!prompt.contains("fall back to bash only when necessary"));

    // Per-tool mechanics still come from each tool's own guidelines.
    assert!(
        prompt.contains("To read or examine files, use `read` instead of cat, head, tail, or sed.")
    );
    assert!(prompt.contains("To edit files, use `edit` instead of sed or awk."));
    assert!(prompt.contains(
        "To create files, use `write` instead of cat with a heredoc or echo redirection."
    ));
    assert!(prompt.contains("Show file paths clearly when working with files"));
}

#[test]
fn available_tools_list_uses_model_resolved_alias_names() {
    use evot_engine::tools::GrepTool;
    use evot_engine::tools::ReadFileTool;
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let tools: Vec<Box<dyn evot_engine::AgentTool>> =
        vec![Box::new(ReadFileTool::default()), Box::new(GrepTool::new())];

    // Claude models are offered the capitalized aliases, so the advertised
    // names in the prompt must match what the model can actually call.
    let claude = SystemPrompt::base(&tmp.path().to_string_lossy(), &tools, "claude-opus-4-6").0;
    assert!(claude.contains("- Read: "), "expected Read alias: {claude}");
    assert!(claude.contains("- Grep: "), "expected Grep alias: {claude}");
    assert!(!claude.contains("- read: "), "base name leaked: {claude}");
    assert!(
        claude.contains("use `Read` instead of"),
        "prefer line should use Read alias: {claude}"
    );

    // Non-Claude models keep the base names.
    let other = SystemPrompt::base(&tmp.path().to_string_lossy(), &tools, "gpt-4o").0;
    assert!(other.contains("- read: "), "expected base name: {other}");
    assert!(
        other.contains("use `read` instead of"),
        "prefer line should use base name: {other}"
    );
}

#[test]
fn dedicated_search_tools_flip_bash_framing() {
    use evot_engine::tools::BashTool;
    use evot_engine::tools::GrepTool;
    use evot_engine::tools::ReadFileTool;
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let tools: Vec<Box<dyn evot_engine::AgentTool>> = vec![
        Box::new(ReadFileTool::default()),
        Box::new(BashTool::default()),
        Box::new(GrepTool::new()),
    ];
    let prompt = SystemPrompt::base(&tmp.path().to_string_lossy(), &tools, "").0;

    assert!(prompt.contains("Do not run a bash command when a dedicated tool exists"));
    assert!(prompt.contains("fall back to bash only when necessary"));
    assert!(!prompt.contains("Use bash for file operations like ls, rg, find"));
}

fn ctx(mode: PromptMode) -> DynamicContext {
    DynamicContext {
        mode,
        sandbox: false,
        variables: Vec::new(),
    }
}

#[test]
fn interactive_mode_adds_language_section_only() {
    let sections = dynamic_sections(&ctx(PromptMode::Interactive));
    assert_eq!(names(&sections), vec!["language"]);
}

#[test]
fn planning_mode_adds_planning_section_only() {
    let sections = dynamic_sections(&ctx(PromptMode::Planning));
    assert_eq!(names(&sections), vec!["planning_mode"]);
}

#[test]
fn headless_and_readonly_add_no_mode_section() {
    assert!(dynamic_sections(&ctx(PromptMode::Headless)).is_empty());
    assert!(dynamic_sections(&ctx(PromptMode::Readonly)).is_empty());
}

#[test]
fn sandbox_and_variables_layer_onto_mode_section() {
    let sections = dynamic_sections(&DynamicContext {
        mode: PromptMode::Headless,
        sandbox: true,
        variables: vec!["TOKEN".to_string(), "REGION".to_string()],
    });
    // Headless contributes no mode section; runtime state still applies.
    assert_eq!(names(&sections), vec!["sandbox", "variables"]);
    let vars = &sections[1].text;
    assert!(vars.contains("TOKEN"));
    assert!(vars.contains("REGION"));
}

#[test]
fn interactive_with_sandbox_orders_mode_before_runtime_state() {
    let sections = dynamic_sections(&DynamicContext {
        mode: PromptMode::Interactive,
        sandbox: true,
        variables: Vec::new(),
    });
    assert_eq!(names(&sections), vec!["language", "sandbox"]);
}
