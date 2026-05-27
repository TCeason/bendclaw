use evot::agent::prompt::SystemPrompt;

fn build_prompt(cwd: &str) -> String {
    SystemPrompt::new(cwd)
        .with_system()
        .with_tools()
        .with_project_context()
        .with_dynamic_boundary()
        .with_git()
        .build()
}

#[test]
fn base_prompt_contains_section_headers() {
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let prompt = build_prompt(&tmp.path().to_string_lossy());
    assert!(prompt.contains("# System"));
    assert!(prompt.contains("# Agent behavior"));
    assert!(prompt.contains("# Guidelines"));
    assert!(prompt.contains("# Environment"));
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
    let guidelines_pos = prompt.find("# Guidelines").expect("missing # Guidelines");
    let env_pos = prompt.find("# Environment").expect("missing # Environment");
    let git_pos = prompt.find("# Git").expect("missing # Git");

    assert!(
        system_pos < agent_pos,
        "# System should come before # Agent behavior"
    );
    assert!(
        agent_pos < guidelines_pos,
        "# Agent behavior should come before # Guidelines"
    );
    assert!(
        guidelines_pos < env_pos,
        "# Guidelines should come before # Environment"
    );
    assert!(env_pos < git_pos, "# Environment should come before # Git");
}
