//! Integration tests for the command runner (`dirsql::command::run_command`).
//!
//! These exercise the **effectful** half of the primitive — real process
//! spawning, timeouts, stdin, and exit handling — which the Rust `unit lint`
//! isolation rule keeps out of colocated unit tests. They are Unix-only (they
//! shell out to `sh`/`cat`/`sleep`); the Rust CI test job runs on Linux.
#![cfg(unix)]

use std::time::Duration;

use dirsql::command::{CommandError, Placeholder, run_command};
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn returns_the_last_non_empty_stdout_line_as_the_payload() {
    let dir = TempDir::new().unwrap();
    let out = run_command(
        "sh -c 'echo starting; echo PAYLOAD; echo'",
        &[],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect("command succeeds");
    assert_eq!(out.payload, "PAYLOAD");
}

#[test]
fn substitutes_a_placeholder_and_reads_the_named_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("data.txt"), "row-a\nrow-b\n").unwrap();
    let out = run_command(
        "cat {path}",
        &[Placeholder::new("path", "data.txt")],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect("command succeeds");
    assert_eq!(out.payload, "row-b");
}

#[test]
fn a_placeholder_the_template_omits_is_not_appended() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("data.txt"), "only-line\n").unwrap();
    // `cat` with no argument reads its (null) stdin: an omitted `{path}` is not
    // appended, so there is no file to read and the run produces no payload.
    let err = run_command(
        "cat",
        &[Placeholder::new("path", "data.txt")],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect_err("no path is appended, so `cat` reads empty stdin");
    assert!(
        matches!(err, CommandError::EmptyOutput { .. }),
        "got: {err:?}"
    );
}

#[test]
fn runs_in_the_given_cwd_so_relative_paths_resolve() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("marker.txt"), "here\n").unwrap();
    let out =
        run_command("cat marker.txt", &[], dir.path(), TIMEOUT, None).expect("command succeeds");
    assert_eq!(out.payload, "here");
}

#[test]
fn inherits_the_parent_environment() {
    let dir = TempDir::new().unwrap();
    let parent_path = std::env::var("PATH").expect("PATH set");
    let out = run_command(
        r#"sh -c 'printf %s "$PATH"'"#,
        &[],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect("command succeeds");
    assert_eq!(out.payload, parent_path);
}

#[test]
fn writes_the_stdin_payload_to_the_child() {
    let dir = TempDir::new().unwrap();
    let out = run_command("cat", &[], dir.path(), TIMEOUT, Some(b"chatter\nPAYLOAD\n"))
        .expect("command succeeds");
    assert_eq!(out.payload, "PAYLOAD");
}

#[test]
fn untrusted_placeholder_values_are_never_shell_interpreted() {
    let dir = TempDir::new().unwrap();
    // The script echoes its first positional arg (`$1`) verbatim; `{args}`
    // arrives as a single argv token, so its metacharacters are inert.
    let out = run_command(
        r#"sh -c 'printf %s "$1"' _ {args}"#,
        &[Placeholder::new("args", "a; rm -rf / && echo pwned")],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect("command succeeds");
    assert_eq!(out.payload, "a; rm -rf / && echo pwned");
}

#[test]
fn non_zero_exit_is_an_error_carrying_the_stderr_tail() {
    let dir = TempDir::new().unwrap();
    let err = run_command(
        "sh -c 'echo boom >&2; exit 3'",
        &[],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect_err("command fails");
    match err {
        CommandError::NonZeroExit {
            code, stderr_tail, ..
        } => {
            assert_eq!(code, "3");
            assert_eq!(stderr_tail, "boom");
        }
        other => panic!("expected NonZeroExit, got {other:?}"),
    }
}

#[test]
fn a_signal_terminated_child_reports_code_signal() {
    let dir = TempDir::new().unwrap();
    let err = run_command("sh -c 'kill -TERM $$'", &[], dir.path(), TIMEOUT, None)
        .expect_err("command fails");
    match err {
        CommandError::NonZeroExit { code, .. } => assert_eq!(code, "signal"),
        other => panic!("expected NonZeroExit(signal), got {other:?}"),
    }
}

#[test]
fn a_child_that_exceeds_the_timeout_is_killed() {
    let dir = TempDir::new().unwrap();
    let err = run_command(
        "sleep 30",
        &[],
        dir.path(),
        Duration::from_millis(150),
        None,
    )
    .expect_err("command times out");
    assert!(
        matches!(err, CommandError::Timeout { .. }),
        "expected Timeout, got {err:?}"
    );
}

#[test]
fn a_missing_program_is_a_spawn_error() {
    let dir = TempDir::new().unwrap();
    let err = run_command(
        "dirsql-no-such-program-xyzzy --nope",
        &[],
        dir.path(),
        TIMEOUT,
        None,
    )
    .expect_err("spawn fails");
    assert!(
        matches!(err, CommandError::Spawn { .. }),
        "expected Spawn, got {err:?}"
    );
}

#[test]
fn a_clean_exit_with_no_output_is_an_empty_output_error() {
    let dir = TempDir::new().unwrap();
    let err = run_command("true", &[], dir.path(), TIMEOUT, None).expect_err("no payload");
    assert!(
        matches!(err, CommandError::EmptyOutput { .. }),
        "expected EmptyOutput, got {err:?}"
    );
}

#[test]
fn an_invalid_template_is_rejected_before_spawning() {
    let dir = TempDir::new().unwrap();
    let err = run_command("   ", &[], dir.path(), TIMEOUT, None).expect_err("invalid");
    assert!(
        matches!(err, CommandError::InvalidCommand(_)),
        "expected InvalidCommand, got {err:?}"
    );
}

#[test]
fn a_large_stdin_payload_flows_without_deadlocking() {
    let dir = TempDir::new().unwrap();
    // Larger than a typical 64KiB pipe buffer, to exercise the concurrent
    // stdin-writer / stdout-reader threads.
    let mut payload = "x".repeat(512 * 1024).into_bytes();
    payload.extend_from_slice(b"\nBIGPAYLOAD\n");
    let out =
        run_command("cat", &[], dir.path(), TIMEOUT, Some(&payload)).expect("command succeeds");
    assert_eq!(out.payload, "BIGPAYLOAD");
}
