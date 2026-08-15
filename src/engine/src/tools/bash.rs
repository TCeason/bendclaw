//! Bash tool — execute shell commands with timeout, streaming output, and process cleanup.

use crate::types::*;

/// Type alias for command confirmation callback.
pub type ConfirmFn = Box<dyn Fn(&str) -> bool + Send + Sync>;
use std::io::Write;
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
    /// Default execution time per command when none is requested.
    pub timeout: Duration,
    /// Hard ceiling on the per-command timeout. A model-requested `timeout`
    /// is clamped to this; no command may exceed it.
    pub max_timeout: Duration,
    /// Cap on the in-memory rolling tail. The spill file holds the complete
    /// output once it exceeds the display limits, so this only bounds memory.
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
            timeout: Duration::from_secs(600),      // 10 minutes
            max_timeout: Duration::from_secs(1800), // 30 minutes hard cap
            max_output_bytes: 256 * 1024,           // 256KB
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

    /// Set the hard ceiling on per-command timeout. Clamped so it is never
    /// below the default timeout.
    pub fn with_max_timeout(mut self, max_timeout: Duration) -> Self {
        self.max_timeout = max_timeout;
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

/// Streaming capture of a command's merged stdout+stderr, mirroring pi's
/// `OutputAccumulator`. Under the display limits everything stays in memory;
/// once crossed, the spill file is opened (buffered bytes replayed into it)
/// and every byte is written through, so the file always holds the complete
/// output while memory keeps only a bounded rolling tail.
struct OutputCapture {
    /// Full output until the spill file opens, then the last `tail_cap` bytes.
    buf: Vec<u8>,
    total_bytes: usize,
    newlines: usize,
    open_line: bool,
    file: Option<std::fs::File>,
    path: Option<PathBuf>,
    spill: Option<Arc<crate::spill::FsSpill>>,
    key: String,
    tail_cap: usize,
}

impl OutputCapture {
    fn new(spill: Option<Arc<crate::spill::FsSpill>>, key: String, tail_cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(4096),
            total_bytes: 0,
            newlines: 0,
            open_line: false,
            file: None,
            path: None,
            spill,
            key,
            tail_cap,
        }
    }

    fn append(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        self.total_bytes += chunk.len();
        self.newlines += chunk.iter().filter(|&&b| b == b'\n').count();
        self.open_line = chunk.last() != Some(&b'\n');

        if self.file.is_none() && self.over_threshold() {
            self.open_spill_file();
        }

        if let Some(file) = self.file.as_mut() {
            let _ = file.write_all(chunk);
        }
        self.buf.extend_from_slice(chunk);
        self.trim_tail();
    }

    /// Flush and close the spill file, if open.
    fn finish(&mut self) {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
        }
    }

    /// Lossy-UTF-8 rolling tail, for display and progress snapshots.
    fn tail_text(&self) -> String {
        String::from_utf8_lossy(&self.buf).to_string()
    }

    fn total_lines(&self) -> usize {
        self.newlines + usize::from(self.open_line)
    }

    fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    fn over_threshold(&self) -> bool {
        self.total_bytes > MAX_DISPLAY_BYTES || self.total_lines() > MAX_DISPLAY_LINES
    }

    fn open_spill_file(&mut self) {
        let Some(spill) = self.spill.clone() else {
            return;
        };
        let path = spill.path_for_key(&self.key);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::File::create(&path) {
            Ok(mut file) => {
                let _ = file.write_all(&self.buf);
                self.file = Some(file);
                self.path = Some(path);
            }
            Err(e) => {
                tracing::warn!("bash full-output spill open failed: {e}");
            }
        }
    }

    /// Drain the front at a newline boundary. Not allowed before the spill
    /// file opens (or before crossing the threshold without a spill store):
    /// the buffer must stay complete for a lossless replay.
    fn trim_tail(&mut self) {
        if self.file.is_none() && !self.over_threshold() {
            return;
        }
        if self.buf.len() > self.tail_cap * 2 {
            let target = self.buf.len() - self.tail_cap;
            let drain_to = self.buf[target..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(target, |p| target + p + 1);
            self.buf.drain(..drain_to);
        }
    }
}

/// Read a pipe to EOF into the shared capture.
async fn read_into<R: tokio::io::AsyncRead + Unpin>(
    mut pipe: R,
    capture: Arc<parking_lot::Mutex<OutputCapture>>,
) {
    let mut tmp = [0u8; 4096];
    loop {
        match pipe.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => capture.lock().append(&tmp[..n]),
            Err(_) => break,
        }
    }
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
         If truncated, full output is saved to a temp file. Optionally provide a timeout in \
         seconds."
    }

    fn prompt_snippet(&self) -> Option<&str> {
        Some("Execute bash commands (ls, grep, find, etc.)")
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
                    "description": "Optional timeout in seconds."
                }
            },
            "required": ["command"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        params["command"].as_str().map(|s| s.to_string())
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
        // A model-requested timeout overrides the default, clamped to
        // [default, max_timeout]. Absent/zero/negative falls back to default.
        let timeout = match params["timeout"].as_f64() {
            Some(secs) if secs > 0.0 => {
                // Clamp to [default, max] without risking a clamp() panic if a
                // caller ever configured default > max.
                Duration::from_secs_f64(secs)
                    .min(self.max_timeout)
                    .max(self.timeout)
            }
            _ => self.timeout,
        };

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

        // Spawn as a process group so we can kill the entire tree on timeout/cancel.
        // On Unix this creates a real process group; on Windows it uses a job object.
        let mut child = cmd
            .group_spawn()
            .map_err(|e| ToolError::Failed(format!("Failed to execute: {e}")))?;

        // Take ownership of stdout/stderr pipes
        let child_stdout = child.inner().stdout.take();
        let child_stderr = child.inner().stderr.take();

        // Merged stdout+stderr capture; spills the complete output to a file
        // once the display limits are crossed.
        let capture = Arc::new(parking_lot::Mutex::new(OutputCapture::new(
            ctx.spill.clone(),
            format!("{}-bash-output", ctx.tool_call_id),
            self.max_output_bytes,
        )));

        let stdout_task = child_stdout.map(|pipe| {
            let capture = capture.clone();
            tokio::spawn(read_into(pipe, capture))
        });
        let stderr_task = child_stderr.map(|pipe| {
            let capture = capture.clone();
            tokio::spawn(read_into(pipe, capture))
        });

        let start = Instant::now();
        let mut last_progress = Instant::now();
        let mut last_update = Instant::now();

        // Helper: kill the process group and drain IO tasks
        async fn kill_and_drain(
            child: &mut command_group::AsyncGroupChild,
            stdout_task: Option<tokio::task::JoinHandle<()>>,
            stderr_task: Option<tokio::task::JoinHandle<()>>,
        ) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = tokio::time::timeout(IO_DRAIN_TIMEOUT, async {
                if let Some(task) = stdout_task {
                    let _ = task.await;
                }
                if let Some(task) = stderr_task {
                    let _ = task.await;
                }
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

                        let (display, spill_path) = {
                            let mut capture = capture.lock();
                            let tail = capture.tail_text();
                            capture.finish();
                            let (display, _, _) = tail_truncate(&truncate_long_lines(&tail));
                            (display, capture.path().cloned())
                        };
                        let mut msg = format!(
                            "Command timed out after {}s",
                            timeout.as_secs()
                        );
                        if !display.is_empty() {
                            msg.push_str("\nLast output:\n");
                            msg.push_str(&display);
                        }
                        if let Some(path) = spill_path {
                            msg.push_str(&format!("\n\n[Full output saved to: {}]", path.display()));
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
                            let snippet = capture.lock().tail_text();
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
            if let Some(task) = stdout_task {
                let _ = task.await;
            }
            if let Some(task) = stderr_task {
                let _ = task.await;
            }
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

        // Readers are drained: finalize the capture and build the display view.
        let (display, raw_tail, total_lines, spill_path) = {
            let mut capture = capture.lock();
            let tail = capture.tail_text();
            capture.finish();
            let total_lines = capture.total_lines();
            let path = capture.path().cloned();
            let (display, _, _) = tail_truncate(&truncate_long_lines(&tail));
            (display, tail, total_lines, path)
        };

        let mut output = display;
        if let Some(path) = &spill_path {
            let shown_lines = output.lines().count();
            let start_line = total_lines.saturating_sub(shown_lines) + 1;
            output.push_str(&format!(
                "\n\n[Showing lines {start_line}-{total_lines} of {total_lines}. Full output: {}]",
                path.display()
            ));
        }

        if exit_code != 0 {
            output = format!("Exit code: {exit_code}\n{output}");
        }

        // Append sandbox hint on sandbox permission failures
        let output = if self.sandbox_dirs.is_some()
            && exit_code != 0
            && (raw_tail.contains("Operation not permitted")
                || raw_tail.contains("Permission denied"))
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
                "full_output_path": spill_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
            }),
            retention: Retention::Normal,
        })
    }
}
