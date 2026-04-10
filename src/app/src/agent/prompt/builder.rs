use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const PROJECT_CONTEXT_FILES: &[&str] = &["BENDCLAW.md", "CLAUDE.md", "AGENTS.md"];
const MAX_GIT_STATUS_CHARS: usize = 2000;

// ---------------------------------------------------------------------------
// Memory constants
// ---------------------------------------------------------------------------

const MEMORY_PROMPT: &str = include_str!("memory.md");
const MAX_SANITIZED_LENGTH: usize = 200;
const MAX_ENTRYPOINT_LINES: usize = 200;
const MAX_ENTRYPOINT_BYTES: usize = 25_000;

/// Builder for assembling the system prompt.
///
/// ```ignore
/// let prompt = SystemPrompt::new("/path/to/project")
///     .with_system()
///     .with_git()
///     .with_tools()
///     .with_project_context()
///     .with_memory()
///     .with_append("Be concise.")
///     .build();
/// ```
pub struct SystemPrompt {
    cwd: String,
    sections: Vec<String>,
}

impl SystemPrompt {
    pub fn new(cwd: &str) -> Self {
        Self {
            cwd: cwd.to_string(),
            sections: vec!["You are a helpful assistant.".into()],
        }
    }

    /// Append system info: working dir, date, platform, shell, OS version.
    pub fn with_system(mut self) -> Self {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let platform = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let shell = detect_shell();

        let mut lines = vec![
            format!("Working directory: {}", self.cwd),
            format!("Today's date: {today}"),
            format!("Platform: {platform} ({arch})"),
            format!("Shell: {shell}"),
        ];

        if let Some(ver) = detect_os_version() {
            lines.push(format!("OS version: {ver}"));
        }

        self.sections
            .push(format!("# System\n\n{}", lines.join("\n")));
        self
    }

    /// Append git repository info: branch, default branch, user, status, recent commits.
    pub fn with_git(mut self) -> Self {
        let is_git = is_git_repo(&self.cwd);

        let mut lines = vec![format!(
            "Git repository: {}",
            if is_git { "yes" } else { "no" }
        )];

        if is_git {
            if let Some(git_info) = collect_git_info(&self.cwd) {
                lines.push(git_info);
            }
        }

        self.sections.push(format!("# Git\n\n{}", lines.join("\n")));
        self
    }

    /// Append available CLI tools (e.g. `gh`).
    pub fn with_tools(mut self) -> Self {
        let mut lines: Vec<String> = Vec::new();

        if has_command("gh") {
            lines.push(
                "GitHub CLI (`gh`): available — prefer `gh` for all GitHub operations \
                 (issues, PRs, API calls, repo info) instead of `curl` or direct API access"
                    .to_string(),
            );
        }

        if !lines.is_empty() {
            self.sections
                .push(format!("# Tools\n\n{}", lines.join("\n")));
        }
        self
    }

    /// Load and append project context from well-known files.
    pub fn with_project_context(mut self) -> Self {
        let mut context = String::new();
        for name in PROJECT_CONTEXT_FILES {
            let path = Path::new(&self.cwd).join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let content = content.trim();
                if !content.is_empty() {
                    if !context.is_empty() {
                        context.push_str("\n\n");
                    }
                    context.push_str(content);
                }
            }
        }
        if !context.is_empty() {
            self.sections
                .push(format!("# Project Instructions\n\n{context}"));
        }
        self
    }

    /// Load memory from bendclaw and Claude Code directories, inject into system prompt.
    /// Bendclaw memory (`~/.evotai/projects/<slug>/memory/`) is read-write.
    /// Claude Code memory (`~/.claude/projects/<slug>/memory/`) is read-only reference.
    pub fn with_memory(mut self) -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok();
        if let Some(section) = build_memory_section(&self.cwd, home.as_deref()) {
            self.sections.push(section);
        }
        self
    }

    /// Load memory with an explicit home directory override.
    #[doc(hidden)]
    pub fn with_memory_home(mut self, home: &str) -> Self {
        if let Some(section) = build_memory_section(&self.cwd, Some(home)) {
            self.sections.push(section);
        }
        self
    }

    /// Append arbitrary text (e.g. user-supplied `--append-system-prompt`).
    pub fn with_append(mut self, text: &str) -> Self {
        self.sections.push(text.to_string());
        self
    }

    /// Consume the builder and produce the final prompt string.
    pub fn build(self) -> String {
        self.sections.join("\n\n")
    }
}

// ---------------------------------------------------------------------------
// System helpers
// ---------------------------------------------------------------------------

fn detect_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(String::from))
        .unwrap_or_else(|| "unknown".into())
}

fn detect_os_version() -> Option<String> {
    if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        run_cmd("uname", &["-sr"])
    } else if cfg!(target_os = "windows") {
        run_cmd("cmd", &["/C", "ver"])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

fn is_git_repo(cwd: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn collect_git_info(cwd: &str) -> Option<String> {
    let branch = run_git(cwd, &["branch", "--show-current"]).unwrap_or_default();
    let default_branch = detect_default_branch(cwd);
    let user = run_git(cwd, &["config", "user.name"]);
    let status = run_git(cwd, &["status", "--short"]);
    let log = run_git(cwd, &["log", "--oneline", "-n", "5"]);

    let mut parts = Vec::new();

    if !branch.is_empty() {
        parts.push(format!("Current branch: {branch}"));
    }
    if let Some(main) = default_branch {
        parts.push(format!("Default branch: {main}"));
    }
    if let Some(u) = user {
        parts.push(format!("Git user: {u}"));
    }
    if let Some(st) = status {
        let st = if st.is_empty() {
            "(clean)".to_string()
        } else if st.len() > MAX_GIT_STATUS_CHARS {
            format!(
                "{}\n... (truncated, run `git status` for full output)",
                &st[..MAX_GIT_STATUS_CHARS]
            )
        } else {
            st
        };
        parts.push(format!("Status:\n{st}"));
    }
    if let Some(l) = log {
        parts.push(format!("Recent commits:\n{l}"));
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn detect_default_branch(cwd: &str) -> Option<String> {
    if let Some(remote_head) = run_git(cwd, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        return remote_head
            .strip_prefix("refs/remotes/origin/")
            .map(String::from);
    }
    for candidate in &["main", "master"] {
        let exists = Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{candidate}"),
            ])
            .current_dir(cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if exists {
            return Some((*candidate).to_string());
        }
    }
    None
}

fn run_git(cwd: &str, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(["--no-optional-locks"])
        .args(args)
        .current_dir(cwd)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Tool detection helpers
// ---------------------------------------------------------------------------

fn has_command(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// General helpers
// ---------------------------------------------------------------------------

fn run_cmd(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Memory helpers
// ---------------------------------------------------------------------------

/// FNV-1a hash for stable cross-platform path hashing.
/// Not cryptographic — used only for path slug uniqueness on overlong paths.
fn stable_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Sanitize a path for use as a directory name.
/// Mirrors Claude Code's path sanitization for common paths:
/// all non-alphanumeric characters become `-`.
/// For paths exceeding 200 chars, truncates with a stable hash suffix.
fn sanitize_for_path(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if sanitized.len() <= MAX_SANITIZED_LENGTH {
        return sanitized;
    }
    format!(
        "{}-{}",
        &sanitized[..MAX_SANITIZED_LENGTH],
        stable_hash(name)
    )
}

/// Find git repo root via `git rev-parse --show-toplevel`, fallback to cwd.
fn find_git_root(cwd: &str) -> String {
    run_git(cwd, &["rev-parse", "--show-toplevel"]).unwrap_or_else(|| cwd.to_string())
}

/// Compute the project slug used for memory directory naming.
fn memory_project_slug(cwd: &str) -> String {
    sanitize_for_path(&find_git_root(cwd))
}

/// `~/.evotai/projects/<slug>/memory/`
fn bendclaw_memory_dir(cwd: &str, home: &str) -> PathBuf {
    let slug = memory_project_slug(cwd);
    PathBuf::from(home)
        .join(".evotai")
        .join("projects")
        .join(slug)
        .join("memory")
}

/// `~/.claude/projects/<slug>/memory/`
/// Returns `None` if the directory does not exist.
fn claude_memory_dir(cwd: &str, home: &str) -> Option<PathBuf> {
    let slug = memory_project_slug(cwd);
    let dir = PathBuf::from(home)
        .join(".claude")
        .join("projects")
        .join(slug)
        .join("memory");
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Read `MEMORY.md` from a directory. Returns `None` if missing or empty.
fn read_memory_entrypoint(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("MEMORY.md")).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_memory_entrypoint(trimmed))
}

/// Truncate `MEMORY.md` content to line and byte limits.
fn truncate_memory_entrypoint(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let line_over = lines.len() > MAX_ENTRYPOINT_LINES;
    let byte_over = content.len() > MAX_ENTRYPOINT_BYTES;

    if !line_over && !byte_over {
        return content.to_string();
    }

    let mut result = if line_over {
        lines[..MAX_ENTRYPOINT_LINES].join("\n")
    } else {
        content.to_string()
    };

    if result.len() > MAX_ENTRYPOINT_BYTES {
        let safe = truncate_to_char_boundary(&result, MAX_ENTRYPOINT_BYTES);
        let cut = safe.rfind('\n').unwrap_or(safe.len());
        result.truncate(cut);
    }

    result.push_str(
        "\n\n> WARNING: MEMORY.md exceeded the load limit and was truncated. \
         Keep index entries concise and move details into topic files.",
    );
    result
}

/// Find the largest byte offset <= `max_bytes` that falls on a UTF-8 char boundary.
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut cut = max_bytes;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    &s[..cut]
}

/// Best-effort directory creation for the bendclaw memory directory.
/// WriteFileTool also does `create_dir_all`, so this is a convenience fallback.
fn ensure_memory_dir(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
}

/// Build the complete `# Memory` section for the system prompt.
/// Returns `None` if `home` is not provided.
fn build_memory_section(cwd: &str, home: Option<&str>) -> Option<String> {
    let home = home?;
    let bendclaw_dir = bendclaw_memory_dir(cwd, home);

    ensure_memory_dir(&bendclaw_dir);

    let bendclaw_content = read_memory_entrypoint(&bendclaw_dir);
    let claude_content = claude_memory_dir(cwd, home).and_then(|d| read_memory_entrypoint(&d));

    let dir_display = bendclaw_dir.display();

    let mut section = format!(
        "# Memory\n\n\
         You have a persistent, file-based memory system at `{dir_display}`.\n\
         Write memories under this directory using the file write tool.\n\n\
         {MEMORY_PROMPT}\n\n\
         ## Bendclaw MEMORY.md\n\n"
    );

    match bendclaw_content {
        Some(content) => section.push_str(&content),
        None => section.push_str(
            "Your MEMORY.md is currently empty. \
             When you save new memories, they will appear here.",
        ),
    }

    if let Some(content) = claude_content {
        section.push_str(
            "\n\n## Claude Code Memory (read-only reference)\n\n\
             The following memory index was loaded from a Claude Code memory directory \
             for reference only. Do not write to, update, or reorganize that directory.\n\n",
        );
        section.push_str(&content);
    }

    Some(section)
}
