use std::path::Path;
use std::process::Command;

use tokio::sync::OnceCell;

use crate::types::CacheControl;
use crate::types::SystemBlock;

static GIT_STATUS_CACHE: OnceCell<String> = OnceCell::const_new();

pub fn get_git_status(cwd: &str) -> String {
    if let Some(cached) = GIT_STATUS_CACHE.get() {
        return cached.clone();
    }

    let status = compute_git_status(cwd);
    let _ = GIT_STATUS_CACHE.set(status.clone());
    status
}

fn compute_git_status(cwd: &str) -> String {
    let mut result = String::new();

    if let Ok(output) = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            result.push_str(&format!("Current branch: {}\n", branch));
        }
    }

    let main_branch = detect_main_branch(cwd);
    result.push_str(&format!("\nMain branch: {}\n", main_branch));

    if let Ok(output) = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(cwd)
        .output()
    {
        let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !user.is_empty() {
            result.push_str(&format!("\nGit user: {}\n", user));
        }
    }

    if let Ok(output) = Command::new("git")
        .args(["status", "--short"])
        .current_dir(cwd)
        .output()
    {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if status.is_empty() {
            result.push_str("\nStatus:\n(clean)\n");
        } else {
            result.push_str(&format!("\nStatus:\n{}\n", status));
        }
    }

    if let Ok(output) = Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(cwd)
        .output()
    {
        let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !log.is_empty() {
            result.push_str(&format!("\nRecent commits:\n{}\n", log));
        }
    }

    result
}

fn detect_main_branch(cwd: &str) -> String {
    for branch in &["main", "master"] {
        if let Ok(output) = Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(cwd)
            .output()
        {
            if output.status.success() {
                return branch.to_string();
            }
        }
    }

    "main".to_string()
}

pub fn build_system_blocks(
    cwd: &str,
    custom_system_prompt: Option<&str>,
    append_system_prompt: Option<&str>,
) -> Vec<SystemBlock> {
    let mut blocks = Vec::new();
    let system_prompt = custom_system_prompt
        .map(ToString::to_string)
        .unwrap_or_else(default_system_prompt);

    blocks.push(SystemBlock {
        block_type: "text".to_string(),
        text: system_prompt,
        cache_control: Some(CacheControl::ephemeral()),
    });

    let git_status = get_git_status(cwd);
    if !git_status.is_empty() {
        blocks.push(SystemBlock {
            block_type: "text".to_string(),
            text: format!("gitStatus: {}", git_status),
            cache_control: None,
        });
    }

    let user_context = get_user_context(cwd);
    if !user_context.is_empty() {
        blocks.push(SystemBlock {
            block_type: "text".to_string(),
            text: user_context,
            cache_control: Some(CacheControl::ephemeral()),
        });
    }

    if let Some(append) = append_system_prompt {
        blocks.push(SystemBlock {
            block_type: "text".to_string(),
            text: append.to_string(),
            cache_control: None,
        });
    }

    blocks
}

fn default_system_prompt() -> String {
    "You are a helpful AI assistant with access to tools for software engineering tasks. \
     Use the available tools to help the user accomplish their goals. \
     Be concise and direct in your responses."
        .to_string()
}

fn get_user_context(cwd: &str) -> String {
    let mut context = String::new();
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    context.push_str(&format!("Current date: {}\n", date));

    for filename in &["AGENT.md", "CLAUDE.md", ".agent/AGENT.md"] {
        let path = Path::new(cwd).join(filename);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                context.push_str(&format!(
                    "\n# Project context from {}\n{}\n",
                    filename, content
                ));
            }
        }
    }

    context
}

pub fn clear_context_cache() {}
