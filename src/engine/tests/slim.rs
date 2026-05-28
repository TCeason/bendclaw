//! Tests for slim: token-saving output post-processing.
//!
//! Uses `serial_test`-style coordination via a mutex because `slim::is_enabled`
//! reads process-wide state (env var + atomic). Tests that toggle the env var
//! share a `Mutex` so they can't race with each other.

use std::sync::Mutex;

use evotengine::tools::slim;
use evotengine::tools::slim::Slimmed;

// Guard against concurrent env-var manipulation across tests.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_slim_enabled<F: FnOnce() -> R, R>(f: F) -> R {
    let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    slim::set_enabled_override(Some(true));
    let out = f();
    slim::set_enabled_override(None);
    out
}

fn with_slim_disabled<F: FnOnce() -> R, R>(f: F) -> R {
    let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    slim::set_enabled_override(Some(false));
    let out = f();
    slim::set_enabled_override(None);
    out
}

fn run(cmd: &str, exit: i32, stdout: &str, stderr: &str) -> Slimmed {
    slim::on_bash(cmd, exit, stdout.to_string(), stderr.to_string())
}

// --------------------------------------------------------------------------
// global switches
// --------------------------------------------------------------------------

#[test]
fn slim_off_passes_through() {
    with_slim_disabled(|| {
        let big_diff = sample_git_diff_large();
        let out = run("git diff HEAD~5", 0, &big_diff, "");
        assert_eq!(out.stats.filter, "off");
        assert_eq!(out.stdout, big_diff);
        assert_eq!(out.stats.original, big_diff.len());
        assert_eq!(out.stats.slimmed, big_diff.len());
    });
}

#[test]
fn raw_error_passes_through() {
    with_slim_enabled(|| {
        // exit != 0 must always passthrough even with slim enabled.
        let out = run("git commit -m fix", 1, "pretend output", "error: nothing");
        assert_eq!(out.stats.filter, "raw_error");
        assert_eq!(out.stdout, "pretend output");
        assert_eq!(out.stderr, "error: nothing");
    });
}

// --------------------------------------------------------------------------
// tail fallback
// --------------------------------------------------------------------------

#[test]
fn tail_folds_unknown_long_output() {
    with_slim_enabled(|| {
        // 30 KB of junk output for an unknown command → tail fallback.
        let body: String = (0..3000).map(|i| format!("line {i}\n")).collect();
        let out = run("unknown-tool", 0, &body, "");
        assert_eq!(out.stats.filter, "tail");
        assert!(out.stdout.contains("<... slimmed "));
        assert!(out.stdout.len() < body.len());
    });
}

#[test]
fn tail_short_output_is_none() {
    with_slim_enabled(|| {
        let body = "hello\nworld\n";
        let out = run("unknown-tool", 0, body, "");
        // No filter actually fired → filter id is "none".
        assert_eq!(out.stats.filter, "none");
        assert_eq!(out.stdout, body);
    });
}

// --------------------------------------------------------------------------
// git diff
// --------------------------------------------------------------------------

#[test]
fn git_diff_compresses_context() {
    with_slim_enabled(|| {
        let text = sample_git_diff_large();
        let out = run("git diff", 0, &text, "");
        assert_eq!(out.stats.filter, "git_diff");
        assert!(out.stdout.contains("diff --git"));
        assert!(out.stdout.contains("@@ "));
        // Change lines must be preserved.
        assert!(out.stdout.contains("+added line 1"));
        assert!(out.stdout.contains("-removed line 1"));
        // Context fold marker should appear for the big unchanged run.
        assert!(out.stdout.contains("unchanged lines"));
        // Must actually save bytes.
        assert!(out.stats.slimmed < out.stats.original);
    });
}

// --------------------------------------------------------------------------
// git log
// --------------------------------------------------------------------------

#[test]
fn git_log_collapses_commits() {
    with_slim_enabled(|| {
        let text = "commit 0123456789abcdef0123456789abcdef01234567\n\
                    Author: Alice <a@x.y>\n\
                    Date:   Mon May 11 10:00:00 2026 +0800\n\
                    \n\
                    first subject\n\
                    \n\
                    body body body\n\
                    \n\
                    commit fedcba9876543210fedcba9876543210fedcba98\n\
                    Author: Bob <b@x.y>\n\
                    Date:   Tue May 12 11:00:00 2026 +0800\n\
                    \n\
                    second subject\n";
        let out = run("git log", 0, text, "");
        assert_eq!(out.stats.filter, "git_log");
        assert!(out.stdout.contains("0123456"));
        assert!(out.stdout.contains("fedcba9"));
        assert!(out.stdout.contains("first subject"));
        assert!(out.stdout.contains("second subject"));
        assert!(!out.stdout.contains("body body body"));
    });
}

// --------------------------------------------------------------------------
// git status
// --------------------------------------------------------------------------

#[test]
fn git_status_folds_long_sections() {
    with_slim_enabled(|| {
        let mut body = String::new();
        body.push_str("On branch main\n");
        body.push_str("Changes not staged for commit:\n");
        for i in 0..80 {
            body.push_str(&format!("\tmodified:   path/to/file_{i}.rs\n"));
        }
        body.push('\n');
        let out = run("git status", 0, &body, "");
        assert_eq!(out.stats.filter, "git_status");
        assert!(out.stdout.contains("Changes not staged"));
        assert!(out.stdout.contains("... +30 more entries"));
    });
}

// --------------------------------------------------------------------------
// ack filters
// --------------------------------------------------------------------------

#[test]
fn git_commit_success_single_line() {
    with_slim_enabled(|| {
        let text = "[main a1b2c3d] fix: handle empty input\n 3 files changed, 42 insertions(+), 8 deletions(-)\n";
        let out = run("git commit -m fix", 0, text, "");
        assert_eq!(out.stats.filter, "ack");
        assert!(out.stdout.starts_with("ok: a1b2c3d 3 files +42 -8"));
        assert!(out.stdout.contains("fix: handle empty input"));
        assert!(out.stats.slimmed < out.stats.original);
    });
}

#[test]
fn git_push_summary() {
    with_slim_enabled(|| {
        let text = "To github.com:foo/bar.git\n   deadbee..cafebab  main -> main\n";
        let out = run("git push", 0, text, "");
        assert_eq!(out.stats.filter, "ack");
        assert!(out.stdout.contains("main -> main"));
        assert!(out.stdout.starts_with("ok: push"));
    });
}

#[test]
fn git_push_up_to_date() {
    with_slim_enabled(|| {
        let out = run("git push", 0, "Everything up-to-date\n", "");
        assert_eq!(out.stats.filter, "ack");
        assert_eq!(out.stdout, "ok: push up-to-date");
    });
}

#[test]
fn git_push_empty_output_is_none() {
    with_slim_enabled(|| {
        let out = run("git push", 0, "", "");
        assert_eq!(out.stats.filter, "none");
        assert_eq!(out.stdout, "");
    });
}

#[test]
fn npm_install_ack() {
    with_slim_enabled(|| {
        let body = r#"
> prepare
> something

added 42 packages in 8s
"#;
        let out = run("npm install", 0, body, "");
        assert_eq!(out.stats.filter, "ack");
        assert!(out.stdout.starts_with("ok: npm added 42 packages in 8s"));
    });
}

// --------------------------------------------------------------------------
// content sniffing + JSON
// --------------------------------------------------------------------------

#[test]
fn content_sniffing_routes_diff_from_unknown_command() {
    with_slim_enabled(|| {
        let text = sample_git_diff_large();
        let out = run("sed -n 1,200p /tmp/change.diff", 0, &text, "");
        assert_eq!(out.stats.filter, "git_diff");
        assert!(out.stdout.contains("diff --git"));
        assert!(out.stdout.contains("+added line 1"));
        assert!(out.stdout.contains("unchanged lines"));
    });
}

#[test]
fn json_filter_compacts_long_strings_and_arrays() {
    with_slim_enabled(|| {
        // Must exceed 32KB gate to trigger JSON compaction.
        let long_body = "x".repeat(33_000);
        let stdout = format!(
            r#"{{"title":"demo","body":"{}","items":[1,2,3,4,5,6,7]}}"#,
            long_body
        );
        assert!(stdout.len() > 32 * 1024);
        let out = run("gh pr view --json title,body,items", 0, &stdout, "");
        assert_eq!(out.stats.filter, "json");
        assert!(out.stdout.contains("title: \"demo\""));
        assert!(out.stdout.contains("body: \""));
        assert!(out.stdout.contains("...\""));
        assert!(out.stdout.contains("items:"));
        assert!(out.stdout.contains("... +6 more"));
        assert!(out.stats.slimmed < out.stats.original);
    });
}

#[test]
fn json_filter_ignores_invalid_json() {
    with_slim_enabled(|| {
        let out = run("unknown-tool", 0, "{not json", "");
        assert_eq!(out.stats.filter, "none");
        assert_eq!(out.stdout, "{not json");
    });
}

#[test]
fn json_filter_keeps_tiny_json_when_compaction_would_expand() {
    with_slim_enabled(|| {
        let out = run("unknown-tool", 0, r#"{"a":1}"#, "");
        assert_eq!(out.stats.filter, "none");
        assert_eq!(out.stdout, r#"{"a":1}"#);
    });
}

#[test]
fn json_filter_skips_moderate_json_under_gate() {
    with_slim_enabled(|| {
        // Simulate a ~7KB JSON response (e.g. tweet thread) — must pass through untouched.
        let body = "x".repeat(6_000);
        let stdout = format!(r#"[{{"id":"123","text":"{}","author":"user"}}]"#, body);
        assert!(stdout.len() < 32 * 1024);
        let out = run("opencli twitter thread 123 -f json", 0, &stdout, "");
        // Should fall through to tail, which also won't fire (under 8KB).
        assert_eq!(out.stats.filter, "none");
        assert_eq!(out.stdout, stdout);
    });
}

#[test]
fn json_filter_escapes_strings() {
    with_slim_enabled(|| {
        // Must exceed 32KB gate to trigger JSON compaction.
        let long = format!(
            r#"{{"msg":"quote \" and newline\n {}","items":[1,2,3,4,5,6,7]}}"#,
            "x".repeat(33_000)
        );
        assert!(long.len() > 32 * 1024);
        let out = run("unknown-tool", 0, &long, "");
        assert_eq!(out.stats.filter, "json");
        assert!(out.stdout.contains(r#"\""#));
        assert!(out.stdout.contains(r#"\n"#));
    });
}

#[test]
fn tail_folds_output_over_line_threshold() {
    with_slim_enabled(|| {
        let body: String = (0..350).map(|i| format!("short {i}\n")).collect();
        assert!(body.len() < 8 * 1024);
        let out = run("unknown-tool", 0, &body, "");
        assert_eq!(out.stats.filter, "tail");
        assert!(out.stdout.contains("<... slimmed "));
    });
}

// --------------------------------------------------------------------------
// fixtures
// --------------------------------------------------------------------------

fn sample_git_diff_large() -> String {
    let mut s = String::new();
    s.push_str("diff --git a/src/foo.rs b/src/foo.rs\n");
    s.push_str("index 1111111..2222222 100644\n");
    s.push_str("--- a/src/foo.rs\n");
    s.push_str("+++ b/src/foo.rs\n");
    s.push_str("@@ -1,40 +1,40 @@\n");
    for i in 0..20 {
        s.push_str(&format!(" unchanged ctx {i}\n"));
    }
    s.push_str("-removed line 1\n");
    s.push_str("+added line 1\n");
    for i in 20..40 {
        s.push_str(&format!(" unchanged ctx {i}\n"));
    }
    s
}
