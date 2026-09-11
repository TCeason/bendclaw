//! A tool child that keeps evot's controlling terminal can `tcsetpgrp()` the
//! foreground onto itself, leaving evot stopped by SIGTTIN — reported by the
//! shell as `suspended (tty input)`.

#![cfg(unix)]

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use evotengine::tools::BashTool;
use evotengine::tools::ProcessManager;
use evotengine::tools::TaskStopTool;
use evotengine::types::AgentTool;

use super::background_process::context;
use super::background_process::task_id;
use super::background_process::text;

const TTY_PROBE: &str = "if : < /dev/tty 2>/dev/null; then echo OPEN; else echo DENIED; fi";

/// The same probe in a child that shares our session — what `setpgid` alone
/// left us with. Tells us whether this runner has a tty, so a green assertion
/// below cannot be mistaken for proof when nothing could reach a tty anyway.
async fn control_probe() -> Result<String, Box<dyn Error>> {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(TTY_PROBE)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tokio::test]
async fn tool_child_cannot_reach_evots_terminal() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let manager = Arc::new(ProcessManager::new());
    let bash = BashTool::new().with_process_manager(manager.clone());

    let result = bash
        .execute(
            serde_json::json!({"command": TTY_PROBE}),
            context("bash", dir.path()),
        )
        .await?;
    let observed = text(&result);
    assert!(
        observed.contains("DENIED"),
        "tool child reached a controlling terminal, so it can steal evot's \
         foreground and suspend it: {observed}"
    );

    if control_probe().await? != "OPEN" {
        eprintln!("note: no tty on this runner; the assertion held but did not discriminate");
    }
    Ok(())
}

/// `setsid` makes the child a session *and* group leader, so the group signal
/// must still reach its children.
#[tokio::test]
async fn stopping_a_task_kills_its_grandchildren() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let manager = Arc::new(ProcessManager::new());
    let bash = BashTool::new().with_process_manager(manager.clone());
    let stop = TaskStopTool::new(manager.clone());
    let marker = dir.path().join("grandchild.pid");

    let started = bash
        .execute(
            serde_json::json!({
                "command": format!("sleep 30 & echo $! > {}; wait", marker.display()),
                "run_in_background": true
            }),
            context("bash", dir.path()),
        )
        .await?;
    let id = task_id(&started)?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let grandchild = loop {
        if let Ok(recorded) = std::fs::read_to_string(&marker) {
            if let Ok(pid) = recorded.trim().parse::<i32>() {
                break pid;
            }
        }
        if Instant::now() >= deadline {
            return Err("grandchild never recorded its pid".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    stop.execute(
        serde_json::json!({"task_id": id}),
        context("task_stop", dir.path()),
    )
    .await?;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // SAFETY: signal 0 only probes for existence.
        if unsafe { libc::kill(grandchild, 0) } == -1 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            // SAFETY: SIGKILL to a pid this test created.
            unsafe { libc::kill(grandchild, libc::SIGKILL) };
            return Err("grandchild survived the group kill".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
