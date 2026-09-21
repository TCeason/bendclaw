//! `reap_exited_children` drains the wait queue ahead of an in-place `execve`.
//!
//! Lives in its own test binary, as a single test, on purpose: `waitpid(-1)`
//! collects any child of the process, so sharing a process with tests that
//! spawn their own children would race with them.

#![cfg(unix)]

use std::error::Error;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use evotengine::tools::process::reap_exited_children;

fn spawn_sh(script: &str) -> Result<std::process::Child, Box<dyn Error>> {
    Ok(Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

#[test]
fn reaps_abandoned_children_within_the_deadline() -> Result<(), Box<dyn Error>> {
    // Dropping `Child` without `wait()` mirrors a hook adapter or a
    // just-killed background task at restart time.
    let already_done = spawn_sh("exit 0")?;
    let finishing_soon = spawn_sh("sleep 0.1")?;
    drop(finishing_soon);
    std::thread::sleep(Duration::from_millis(50));
    drop(already_done);

    let reaped = reap_exited_children(Duration::from_secs(2));
    assert_eq!(
        reaped, 2,
        "both children must be collected within the deadline"
    );

    // Nothing is left, so a second pass returns immediately.
    let started = Instant::now();
    assert_eq!(reap_exited_children(Duration::from_secs(1)), 0);
    assert!(started.elapsed() < Duration::from_millis(200));

    // A child that keeps running only costs the deadline, never blocks.
    let mut stubborn = spawn_sh("sleep 5")?;

    let started = Instant::now();
    let reaped = reap_exited_children(Duration::from_millis(100));
    let elapsed = started.elapsed();
    assert_eq!(reaped, 0);
    assert!(
        elapsed >= Duration::from_millis(100),
        "returned before the deadline: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "overshot the deadline: {elapsed:?}"
    );

    stubborn.kill()?;
    stubborn.wait()?;
    Ok(())
}
