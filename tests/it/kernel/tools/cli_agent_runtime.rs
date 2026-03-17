use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::Result;
use bendclaw::kernel::tools::cli_agent::AgentOptions;
use bendclaw::kernel::tools::cli_agent::AgentProcess;
use bendclaw::kernel::tools::cli_agent::AgentStateKey;
use bendclaw::kernel::tools::cli_agent::CliAgent;
use bendclaw::kernel::tools::cli_agent::CliAgentState;
use tempfile::tempdir;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

struct ScriptAgent {
    bin: String,
    stdin_followup: bool,
}

impl CliAgent for ScriptAgent {
    fn agent_type(&self) -> &str {
        "script-agent"
    }

    fn command_name(&self) -> &str {
        &self.bin
    }

    fn build_command(&self, cwd: &Path, prompt: &str, _opts: &AgentOptions) -> Command {
        let mut cmd = self.base_command();
        cmd.current_dir(cwd);
        if !prompt.is_empty() {
            cmd.arg(prompt);
        }
        cmd
    }

    fn build_resume_command(
        &self,
        cwd: &Path,
        _session_id: &str,
        prompt: &str,
        opts: &AgentOptions,
    ) -> Command {
        self.build_command(cwd, prompt, opts)
    }

    fn parse_session_id(&self, line: &serde_json::Value) -> Option<String> {
        line.get("session_id")?.as_str().map(ToString::to_string)
    }

    fn parse_events(
        &self,
        _line: &serde_json::Value,
    ) -> Vec<bendclaw::kernel::tools::cli_agent::AgentEvent> {
        vec![]
    }

    fn parse_result(&self, line: &serde_json::Value) -> Option<String> {
        line.get("result")?.as_str().map(ToString::to_string)
    }

    fn supports_stdin_followup(&self) -> bool {
        self.stdin_followup
    }

    fn build_stdin_message(&self, prompt: &str) -> Option<String> {
        self.stdin_followup.then(|| format!("{prompt}\n"))
    }
}

fn write_script(contents: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = tempdir()?;
    let script = dir.path().join("mock-agent.sh");
    fs::write(&script, contents)?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms)?;
    Ok((dir, script.to_string_lossy().to_string()))
}

#[test]
fn cli_agent_state_is_scoped_by_agent_and_working_dir() {
    let mut state = CliAgentState::new();
    let a = AgentStateKey::new("claude", "/tmp/a");
    let b = AgentStateKey::new("claude", "/tmp/b");
    let c = AgentStateKey::new("codex", "/tmp/a");

    state.set_session_id(a.clone(), "sid-a".into());
    state.set_session_id(b.clone(), "sid-b".into());
    state.set_session_id(c.clone(), "sid-c".into());

    assert_eq!(state.get_session_id(&a), Some("sid-a"));
    assert_eq!(state.get_session_id(&b), Some("sid-b"));
    assert_eq!(state.get_session_id(&c), Some("sid-c"));
}

#[tokio::test]
async fn read_until_result_includes_stderr_tail_in_error() -> Result<()> {
    let (_dir, script) = write_script(
        r#"#!/bin/sh
printf 'first stderr\n' >&2
printf 'not-json\n'
printf 'second stderr\n' >&2
exit 0
"#,
    )?;
    let cwd = tempdir()?;
    let agent = ScriptAgent {
        bin: script,
        stdin_followup: false,
    };

    let mut process = AgentProcess::spawn(&agent, cwd.path(), "", &AgentOptions::default()).await?;
    let err = process
        .read_until_result(&agent, None, "tc_1", &CancellationToken::new())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("exited without a result message"));
    assert!(msg.contains("first stderr"));
    assert!(msg.contains("second stderr"));
    Ok(())
}

#[tokio::test]
async fn start_sends_prompt_via_stdin_for_followup_agents() -> Result<()> {
    let (_dir, script) = write_script(
        r#"#!/bin/sh
IFS= read -r line
printf '{"session_id":"s1"}\n'
printf '{"result":"%s"}\n' "$line"
"#,
    )?;
    let cwd = tempdir()?;
    let agent = ScriptAgent {
        bin: script,
        stdin_followup: true,
    };

    let mut process = AgentProcess::start(
        &agent,
        cwd.path(),
        "hello from stdin",
        &AgentOptions::default(),
    )
    .await?;
    let result = process
        .read_until_result(&agent, None, "tc_1", &CancellationToken::new())
        .await?;
    assert_eq!(result, "hello from stdin");
    assert_eq!(process.session_id(), Some("s1"));
    Ok(())
}
