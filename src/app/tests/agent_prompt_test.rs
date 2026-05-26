use evot::agent::prompt::SystemPrompt;

fn build_prompt(cwd: &str) -> String {
    SystemPrompt::new(cwd)
        .with_system_guidance()
        .with_agent_behavior()
        .with_tool_guidance()
        .with_tone_and_style()
        .with_output_format()
        .with_clarifying_questions()
        .with_output_efficiency()
        .with_context_management()
        .with_environment_static()
        .with_tools()
        .with_project_context()
        .with_dynamic_boundary()
        .with_today_date()
        .with_git()
        .build()
}

#[test]
fn no_context_files_produces_base_prompt_with_system() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("# System"));
    assert!(prompt.contains("# Agent behavior"));
    assert!(prompt.contains("# Using your tools"));
    assert!(prompt.contains("# Tone and style"));
    assert!(prompt.contains("# Output format"));
    assert!(prompt.contains("__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__"));
    assert!(prompt.contains("# Clarifying questions"));
    assert!(prompt.contains("read-only investigation"));
    assert!(prompt.contains("denied or blocked, adjust your approach"));
    assert!(prompt.contains("# Output efficiency"));
    assert!(prompt.contains("# Context management"));
    assert!(prompt.contains("# Environment"));
    assert!(prompt
        .contains("displayed to the user as GitHub-flavored markdown rendered with the CommonMark specification in a monospace terminal"));
    assert!(prompt.contains("GitHub-flavored markdown"));
    assert!(prompt.contains("prompt injection"));
    assert!(prompt.contains("Use Bash for grep, rg, find, ls"));
    assert!(prompt.contains("Use Edit for precise changes"));
    assert!(prompt.contains("old_text must match the file exactly"));
    assert!(prompt.contains(
        "When reading multiple files or running independent commands, make parallel tool calls"
    ));
    assert!(prompt.contains("Act on your best judgment rather than asking for confirmation"));
    assert!(prompt.contains("inspect the relevant existing code before choosing one"));
    assert!(prompt.contains("understand the existing code before suggesting any changes"));
    assert!(prompt.contains("Keep solutions simple and targeted"));
    assert!(prompt.contains("Communication style"));
    assert!(prompt.contains("Do not use a colon before tool calls."));
    assert!(prompt.contains("Keep your text output brief and direct"));
    assert!(prompt.contains("`file_path:line_number`"));
    assert!(prompt.contains("Go straight to the point"));
    assert!(prompt.contains("original tool result may be cleared later"));
    assert!(prompt.contains("Working directory:"));
    assert!(prompt.contains("Today's date:"));
    assert!(prompt.contains("Platform:"));
    assert!(prompt.contains("Shell:"));
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
        .with_tools()
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
    let system_pos = prompt.find("# System").expect("missing # System");
    let agent_pos = prompt
        .find("# Agent behavior")
        .expect("missing # Agent behavior");
    let tools_guidance_pos = prompt
        .find("# Using your tools")
        .expect("missing # Using your tools");
    let tone_pos = prompt
        .find("# Tone and style")
        .expect("missing # Tone and style");
    let output_pos = prompt
        .find("# Output format")
        .expect("missing # Output format");
    let boundary_pos = prompt
        .find("__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__")
        .expect("missing dynamic boundary");
    let date_pos = prompt.find("# Date").expect("missing # Date");
    let clarifying_pos = prompt
        .find("# Clarifying questions")
        .expect("missing # Clarifying questions");
    let text_output_pos = prompt
        .find("# Output efficiency")
        .expect("missing # Output efficiency");
    let context_pos = prompt
        .find("# Context management")
        .expect("missing # Context management");
    let env_pos = prompt.find("# Environment").expect("missing # Environment");
    let git_pos = prompt.find("# Git").expect("missing # Git");

    assert!(
        system_pos < agent_pos,
        "# System should come before # Agent behavior"
    );
    assert!(
        agent_pos < tools_guidance_pos,
        "# Agent behavior should come before # Using your tools"
    );
    assert!(
        tools_guidance_pos < tone_pos,
        "# Using your tools should come before # Tone and style"
    );
    assert!(
        tone_pos < output_pos,
        "# Tone and style should come before # Output format"
    );
    assert!(
        output_pos < clarifying_pos,
        "# Output format should come before # Clarifying questions"
    );
    assert!(
        clarifying_pos < text_output_pos,
        "# Clarifying questions should come before # Output efficiency"
    );
    assert!(
        text_output_pos < context_pos,
        "# Output efficiency should come before # Context management"
    );
    assert!(
        context_pos < env_pos,
        "# Context management should come before # Environment"
    );
    assert!(
        env_pos < boundary_pos,
        "# Environment should come before dynamic boundary"
    );
    assert!(
        boundary_pos < date_pos,
        "dynamic boundary should come before # Date"
    );
    assert!(date_pos < git_pos, "# Date should come before # Git");
}
