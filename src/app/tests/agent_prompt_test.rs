use evot::agent::prompt::SystemPrompt;

fn build_prompt(cwd: &str) -> String {
    SystemPrompt::new(cwd)
        .with_system()
        .with_project_context()
        .with_dynamic_boundary()
        .with_git()
        .build()
}

#[test]
fn base_prompt_contains_section_headers() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("Using your tools:"));
    assert!(prompt.contains("Current working directory:"));
    assert!(prompt.contains("Git repository: no"));
    assert!(!prompt.contains("Project Instructions"));
}

#[test]
fn reads_single_context_file() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("EVOT.md"), "# My Project\nDo X.")
        .expect("failed to write file");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("Project Instructions"));
    assert!(prompt.contains("My Project"));
}

#[test]
fn concatenates_multiple_context_files() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("EVOT.md"), "part one").expect("failed to write file");
    std::fs::write(tmp.path().join("CLAUDE.md"), "part two").expect("failed to write file");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("part one"));
    assert!(prompt.contains("part two"));
}

#[test]
fn skips_empty_context_files() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    std::fs::write(tmp.path().join("EVOT.md"), "   ").expect("failed to write file");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(!prompt.contains("Project Instructions"));
}

#[test]
fn append_is_included() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = SystemPrompt::new(&tmp.path().to_string_lossy())
        .with_environment()
        .with_git()
        .with_project_context()
        .with_append("Be concise.")
        .build();
    assert!(prompt.contains("Be concise."));
}

#[test]
fn git_repo_detected() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let cwd = tmp.path().to_string_lossy().to_string();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to run git init");

    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to set git email");

    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to set git user");

    let prompt = build_prompt(&cwd);
    assert!(prompt.contains("# Git"));
    assert!(prompt.contains("Git repository: yes"));
    assert!(prompt.contains("Git user: Test User"));
}

#[test]
fn git_repo_shows_branch_and_status() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let cwd = tmp.path().to_string_lossy().to_string();

    for (args, _msg) in [
        (vec!["init", "-b", "main"], "init"),
        (vec!["config", "user.email", "test@test.com"], "email"),
        (vec!["config", "user.name", "Tester"], "name"),
    ] {
        std::process::Command::new("git")
            .args(&args)
            .current_dir(&cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git command failed");
    }

    std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write failed");

    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git add failed");

    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(&cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("git commit failed");

    let prompt = build_prompt(&cwd);
    assert!(prompt.contains("Current branch: main"));
    assert!(prompt.contains("Recent commits:"));
    assert!(prompt.contains("initial commit"));
}

#[test]
fn sections_are_ordered_static_then_dynamic() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    let guidelines_pos = prompt
        .find("Using your tools:")
        .expect("missing Using your tools:");
    let cwd_pos = prompt
        .find("Current working directory:")
        .expect("missing Current working directory:");
    let boundary_pos = prompt
        .find("__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__")
        .expect("missing dynamic boundary");
    let git_pos = prompt.find("# Git").expect("missing # Git");

    assert!(
        guidelines_pos < cwd_pos,
        "Guidelines should come before cwd"
    );
    assert!(
        cwd_pos < boundary_pos,
        "cwd should come before dynamic boundary"
    );
    assert!(
        boundary_pos < git_pos,
        "dynamic boundary should come before # Git"
    );
}

#[test]
fn tool_set_drives_identity_list_and_guidelines() {
    use evot_engine::tools::BashTool;
    use evot_engine::tools::EditFileTool;
    use evot_engine::tools::ReadFileTool;
    use evot_engine::tools::WriteFileTool;
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let tools: Vec<Box<dyn evot_engine::AgentTool>> = vec![
        Box::new(ReadFileTool::default()),
        Box::new(BashTool::default()),
        Box::new(EditFileTool::new()),
        Box::new(WriteFileTool::new()),
    ];
    let prompt = SystemPrompt::with_tool_set(&tmp.path().to_string_lossy(), &tools)
        .with_system()
        .build();

    // Identity "Available tools" list is derived from each tool's snippet.
    assert!(prompt.contains("Available tools:"));
    assert!(prompt.contains("- read: Read file contents"));
    assert!(prompt.contains("- write: Create or overwrite files"));

    // Guidelines section opens with the framing principle, then per-tool
    // "prefer this dedicated tool" lines, the bash fallback rule, mechanics,
    // and the shared trailer.
    assert!(prompt.contains("Do not run a bash command when a dedicated tool exists"));
    assert!(
        prompt.contains("To read or examine files, use `read` instead of cat, head, tail, or sed.")
    );
    assert!(prompt.contains("To edit files, use `edit` instead of sed or awk."));
    assert!(prompt.contains(
        "To create files, use `write` instead of cat with a heredoc or echo redirection."
    ));
    assert!(prompt.contains("fall back to bash only when necessary"));
    assert!(prompt.contains("Use edit for precise changes (edits[].oldText must match exactly)"));
    assert!(prompt.contains("Use write only for new files or complete rewrites."));
    assert!(prompt.contains("run in parallel"));
    assert!(prompt.contains("Be concise in your responses"));

    // The legacy snake_case spelling must not leak back in.
    assert!(!prompt.contains("old_text"));
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
    let claude = SystemPrompt::with_tool_set_for_model(
        &tmp.path().to_string_lossy(),
        &tools,
        "claude-opus-4-6",
    )
    .with_system()
    .build();
    assert!(claude.contains("- Read: "), "expected Read alias: {claude}");
    assert!(claude.contains("- Grep: "), "expected Grep alias: {claude}");
    assert!(!claude.contains("- read: "), "base name leaked: {claude}");
    assert!(!claude.contains("- grep: "), "base name leaked: {claude}");
    // The "prefer this tool" guidance must use the same alias the model sees.
    assert!(
        claude.contains("use `Read` instead of"),
        "prefer line should use Read alias: {claude}"
    );
    assert!(
        claude.contains("use `Grep` instead of"),
        "prefer line should use Grep alias: {claude}"
    );

    // Non-Claude models keep the base names.
    let other =
        SystemPrompt::with_tool_set_for_model(&tmp.path().to_string_lossy(), &tools, "gpt-4o")
            .with_system()
            .build();
    assert!(other.contains("- read: "), "expected base name: {other}");
    assert!(other.contains("- grep: "), "expected base name: {other}");
    assert!(
        other.contains("use `read` instead of"),
        "prefer line should use base name: {other}"
    );
}
