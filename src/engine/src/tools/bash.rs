//! Bash tool — execute shell commands with timeout, streaming output, and process cleanup.

use crate::types::*;

/// Type alias for command confirmation callback.
pub type ConfirmFn = Box<dyn Fn(&str) -> bool + Send + Sync>;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use command_group::AsyncCommandGroup;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Execute shell commands. Captures stdout + stderr with streaming progress.
pub struct BashTool {
    /// Working directory for commands
    pub cwd: Option<String>,
    /// Max execution time per command
    pub timeout: Duration,
    /// Max output bytes to capture (prevents OOM on huge outputs)
    pub max_output_bytes: usize,
    /// Commands/patterns that are always blocked (e.g., "rm -rf /")
    pub deny_patterns: Vec<String>,
    /// Optional callback for confirming dangerous commands
    pub confirm_fn: Option<ConfirmFn>,
    /// Environment variables injected into every bash subprocess.
    pub envs: Vec<(String, String)>,
    /// Directories the OS sandbox allows the child process to access.
    /// When set, OS-level sandbox (Seatbelt/Landlock) is applied before exec.
    /// Separate from PathGuard — may include toolchain dirs that file tools should not access.
    pub sandbox_dirs: Option<Vec<PathBuf>>,
}

impl Default for BashTool {
    fn default() -> Self {
        Self {
            cwd: None,
            timeout: Duration::from_secs(600), // 10 minutes
            max_output_bytes: 256 * 1024,      // 256KB
            deny_patterns: vec![
                "rm -rf /".into(),
                "rm -rf /*".into(),
                "mkfs".into(),
                "dd if=".into(),
                ":(){:|:&};:".into(), // fork bomb
            ],
            confirm_fn: None,
            envs: Vec::new(),
            sandbox_dirs: None,
        }
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_deny_patterns(mut self, patterns: Vec<String>) -> Self {
        self.deny_patterns = patterns;
        self
    }

    pub fn with_confirm(mut self, f: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.confirm_fn = Some(Box::new(f));
        self
    }

    pub fn with_envs(mut self, envs: impl IntoIterator<Item = (String, String)>) -> Self {
        self.envs = envs.into_iter().collect();
        self
    }

    pub fn with_sandbox_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.sandbox_dirs = Some(dirs);
        self
    }
}

/// Max lines to include in the final tool result.
const MAX_DISPLAY_LINES: usize = 2000;
/// Max bytes to include in the final tool result.
const MAX_DISPLAY_BYTES: usize = 50 * 1024; // 50KB

/// Extract the last N lines from a byte buffer (up to `max_bytes`).
fn tail_lines(buf: &[u8], max_lines: usize, max_bytes: usize) -> String {
    let text = String::from_utf8_lossy(buf);
    let start = if text.len() > max_bytes {
        text.ceil_char_boundary(text.len() - max_bytes)
    } else {
        0
    };
    let lines: Vec<&str> = text[start..].lines().collect();
    let skip = lines.len().saturating_sub(max_lines);
    lines[skip..].join("\n")
}

/// Tail-truncate output: keep last `MAX_DISPLAY_LINES` / `MAX_DISPLAY_BYTES`.
/// Returns (truncated_text, was_truncated, total_lines).
fn tail_truncate(text: &str) -> (String, bool, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();

    if text.len() <= MAX_DISPLAY_BYTES && total_lines <= MAX_DISPLAY_LINES {
        return (text.to_string(), false, total_lines);
    }

    // Work backwards: collect lines that fit within both limits
    let mut collected: Vec<&str> = Vec::new();
    let mut byte_count = 0usize;

    for &line in lines.iter().rev() {
        let line_bytes = line.len() + 1; // +1 for newline
        if byte_count + line_bytes > MAX_DISPLAY_BYTES || collected.len() >= MAX_DISPLAY_LINES {
            break;
        }
        collected.push(line);
        byte_count += line_bytes;
    }

    collected.reverse();
    (collected.join("\n"), true, total_lines)
}

/// Max bytes per single output line before truncation.
const MAX_LINE_BYTES: usize = 4096;

/// Truncate lines that exceed `MAX_LINE_BYTES`, keeping a head+tail preview.
fn truncate_long_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            result.push('\n');
        }
        if line.len() <= MAX_LINE_BYTES {
            result.push_str(line);
        } else {
            let half = MAX_LINE_BYTES / 2;
            // Find safe char boundaries
            let head_end = line.floor_char_boundary(half);
            let tail_start = line.ceil_char_boundary(line.len().saturating_sub(half));
            let omitted = line.len() - head_end - (line.len() - tail_start);
            result.push_str(&line[..head_end]);
            result.push_str(&format!(" ... ({omitted} bytes truncated) ... "));
            result.push_str(&line[tail_start..]);
        }
    }
    result
}

/// Interval between progress updates.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(3);
/// Interval between partial output updates.
const UPDATE_INTERVAL: Duration = Duration::from_secs(2);
/// Max lines in timeout error last-output summary.
const TIMEOUT_SUMMARY_LINES: usize = 10;
/// Max bytes in timeout error last-output summary.
const TIMEOUT_SUMMARY_BYTES: usize = 2048;
/// Time to wait for IO drain after killing a child.
const IO_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[async_trait]
impl AgentTool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn name_aliases(&self) -> Vec<(String, String)> {
        vec![("claude".into(), "Bash".into())]
    }

    fn label(&self) -> &str {
        "Execute Command"
    }

    fn description(&self) -> &str {
        "Execute a bash command in the current working directory. Returns stdout and stderr. \
         Output is truncated to last 2000 lines or 50KB (whichever is hit first). \
         If truncated, full output is saved to a temp file. Optionally provide a timeout in seconds."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Bash command to execute"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in seconds (optional, no default timeout)"
                }
            },
            "required": ["command"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        params["command"].as_str().map(|s| s.to_string())
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let cancel = ctx.cancel;
        let command = params["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'command' parameter".into()))?;
        let _timeout = params["timeout"].as_f64();

        // Check deny patterns
        for pattern in &self.deny_patterns {
            if command.contains(pattern.as_str()) {
                return Err(ToolError::Failed(format!(
                    "Command blocked by safety policy: contains '{}'. \
                     This pattern is denied for safety.",
                    pattern
                )));
            }
        }

        // Check confirmation callback
        if let Some(ref confirm) = self.confirm_fn {
            if !confirm(command) {
                return Err(ToolError::Failed(
                    "Command was not confirmed by the user.".into(),
                ));
            }
        }

        // Early cancel check
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(command);

        if let Some(ref cwd) = self.cwd {
            cmd.current_dir(cwd);
        }

        if !self.envs.is_empty() {
            cmd.envs(self.envs.iter().map(|(k, v)| (k, v)));
        }

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Apply OS-level sandbox if sandbox_dirs is set
        if let Some(ref dirs) = self.sandbox_dirs {
            super::sandbox::wrap_command(&mut cmd, dirs)
                .map_err(|e| ToolError::Failed(format!("Sandbox setup failed: {e}")))?;
        }

        let timeout = self.timeout;
        let max_bytes = self.max_output_bytes;

        // Spawn as a process group so we can kill the entire tree on timeout/cancel.
        // On Unix this creates a real process group; on Windows it uses a job object.
        let mut child = cmd
            .group_spawn()
            .map_err(|e| ToolError::Failed(format!("Failed to execute: {e}")))?;

        // Take ownership of stdout/stderr pipes
        let child_stdout = child.inner().stdout.take();
        let child_stderr = child.inner().stderr.take();

        // Shared buffers for concurrent reading
        let stdout_buf = Arc::new(parking_lot::Mutex::new(Vec::<u8>::with_capacity(4096)));
        let stderr_buf = Arc::new(parking_lot::Mutex::new(Vec::<u8>::with_capacity(4096)));

        // Spawn stdout reader task — tail-capture: keeps last max_bytes
        let stdout_buf_ref = stdout_buf.clone();
        let stdout_max = max_bytes;
        let stdout_task = tokio::spawn(async move {
            if let Some(mut pipe) = child_stdout {
                let mut tmp = [0u8; 4096];
                loop {
                    match pipe.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut buf = stdout_buf_ref.lock();
                            buf.extend_from_slice(&tmp[..n]);
                            // Trim front when buffer exceeds 2x limit, drain to next newline
                            if buf.len() > stdout_max * 2 {
                                let target = buf.len() - stdout_max;
                                let drain_to = buf[target..]
                                    .iter()
                                    .position(|&b| b == b'\n')
                                    .map_or(target, |p| target + p + 1);
                                buf.drain(..drain_to);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        // Spawn stderr reader task — tail-capture: keeps last max_bytes
        let stderr_buf_ref = stderr_buf.clone();
        let stderr_max = max_bytes;
        let stderr_task = tokio::spawn(async move {
            if let Some(mut pipe) = child_stderr {
                let mut tmp = [0u8; 4096];
                loop {
                    match pipe.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut buf = stderr_buf_ref.lock();
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.len() > stderr_max * 2 {
                                let target = buf.len() - stderr_max;
                                let drain_to = buf[target..]
                                    .iter()
                                    .position(|&b| b == b'\n')
                                    .map_or(target, |p| target + p + 1);
                                buf.drain(..drain_to);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        });

        let start = Instant::now();
        let mut last_progress = Instant::now();
        let mut last_update = Instant::now();

        // Helper: kill the process group and drain IO tasks
        async fn kill_and_drain(
            child: &mut command_group::AsyncGroupChild,
            stdout_task: tokio::task::JoinHandle<()>,
            stderr_task: tokio::task::JoinHandle<()>,
        ) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = tokio::time::timeout(IO_DRAIN_TIMEOUT, async {
                let _ = stdout_task.await;
                let _ = stderr_task.await;
            })
            .await;
        }

        // Main loop: wait for child exit, cancel, or timeout.
        // Periodically send progress/update callbacks.
        let exit_status = loop {
            let next_tick = Duration::from_millis(500);

            tokio::select! {
                _ = cancel.cancelled() => {
                    kill_and_drain(&mut child, stdout_task, stderr_task).await;
                    return Err(ToolError::Cancelled);
                }
                _ = tokio::time::sleep(next_tick) => {
                    let elapsed = start.elapsed();

                    // Check timeout
                    if elapsed >= timeout {
                        kill_and_drain(&mut child, stdout_task, stderr_task).await;

                        let summary = {
                            let buf = stdout_buf.lock();
                            tail_lines(&buf, TIMEOUT_SUMMARY_LINES, TIMEOUT_SUMMARY_BYTES)
                        };
                        let mut msg = format!(
                            "Command timed out after {}s",
                            timeout.as_secs()
                        );
                        if !summary.is_empty() {
                            msg.push_str("\nLast output:\n");
                            msg.push_str(&summary);
                        }
                        return Err(ToolError::Failed(msg));
                    }

                    // Send progress update
                    if elapsed > PROGRESS_INTERVAL
                        && last_progress.elapsed() >= PROGRESS_INTERVAL
                    {
                        if let Some(ref on_progress) = ctx.on_progress {
                            on_progress(format!("Running... {}s", elapsed.as_secs()));
                        }
                        last_progress = Instant::now();
                    }

                    // Send partial output update
                    if elapsed > UPDATE_INTERVAL && last_update.elapsed() >= UPDATE_INTERVAL {
                        if let Some(ref on_update) = ctx.on_update {
                            let snippet = {
                                let buf = stdout_buf.lock();
                                String::from_utf8_lossy(&buf).to_string()
                            };
                            if !snippet.is_empty() {
                                on_update(ToolResult {
                                    content: vec![Content::Text { text: snippet }],
                                    details: serde_json::Value::Null,
                                    retention: Retention::Normal,
                                });
                            }
                        }
                        last_update = Instant::now();
                    }
                }
                status = child.wait() => {
                    break status;
                }
            }
        };

        // Child exited — wait for IO tasks to finish (bounded)
        let _ = tokio::time::timeout(IO_DRAIN_TIMEOUT, async {
            let _ = stdout_task.await;
            let _ = stderr_task.await;
        })
        .await;

        let exit_code = match exit_status {
            Ok(status) => status.code().unwrap_or(-1),
            Err(e) => {
                return Err(ToolError::Failed(format!(
                    "Failed to wait for process: {e}"
                )));
            }
        };

        let mut stdout = {
            let buf = stdout_buf.lock();
            String::from_utf8_lossy(&buf).to_string()
        };
        let mut stderr = {
            let buf = stderr_buf.lock();
            String::from_utf8_lossy(&buf).to_string()
        };

        // Truncate individual long lines (e.g. binary/base64 blobs)
        stdout = truncate_long_lines(&stdout);
        stderr = truncate_long_lines(&stderr);

        // Tail-truncate: keep last 2000 lines / 50KB, discard earlier output
        let (stdout, stdout_truncated, stdout_total) = tail_truncate(&stdout);
        let (stderr, stderr_truncated, stderr_total) = tail_truncate(&stderr);

        // Slim: post-process output for token savings. Disabled commands and
        // `exit != 0` pass through untouched; the sandbox hint below still
        // fires based on the (possibly slimmed) stderr contents.
        let slim_result = super::slim::on_bash(command, exit_code, stdout, stderr);
        let stdout = slim_result.stdout;
        let stderr = slim_result.stderr;
        let slim_stats = slim_result.stats;

        let output = if stderr.is_empty() {
            if stdout_truncated {
                format!(
                    "Exit code: {}\n[Output truncated: showing last {} of {} lines]\n{}",
                    exit_code,
                    stdout.lines().count(),
                    stdout_total,
                    stdout
                )
            } else {
                format!("Exit code: {}\n{}", exit_code, stdout)
            }
        } else {
            let mut out = format!("Exit code: {}\n", exit_code);
            if stdout_truncated {
                out.push_str(&format!(
                    "STDOUT [truncated: showing last {} of {} lines]:\n{}\n",
                    stdout.lines().count(),
                    stdout_total,
                    stdout
                ));
            } else {
                out.push_str(&format!("STDOUT:\n{}\n", stdout));
            }
            if stderr_truncated {
                out.push_str(&format!(
                    "STDERR [truncated: showing last {} of {} lines]:\n{}",
                    stderr.lines().count(),
                    stderr_total,
                    stderr
                ));
            } else {
                out.push_str(&format!("STDERR:\n{}", stderr));
            }
            out
        };

        // Append sandbox hint when command fails with permission errors
        let output = if self.sandbox_dirs.is_some()
            && exit_code != 0
            && (stderr.contains("Operation not permitted") || stderr.contains("Permission denied"))
        {
            format!(
                "{output}\n\n[Sandbox] This command failed due to OS-level sandbox restrictions. \
                 File access is limited to the allowed directories. \
                 Do not retry — the restriction is enforced by the kernel."
            )
        } else {
            output
        };

        // Return output even on failure — LLMs need error output to self-correct
        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({
                "exit_code": exit_code,
                "success": exit_code == 0,
                "slim": slim_stats,
            }),
            retention: Retention::Normal,
        })
    }
}
