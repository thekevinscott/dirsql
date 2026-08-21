//! End-to-end tests for the piped (non-TTY) half of bare `dirsql` (#987).
//!
//! These spawn the real compiled `dirsql` binary with **no subcommand and no
//! SQL**, feed statements on stdin, and assert what only the process boundary
//! can show: the exit code, what reaches stdout, and what reaches stderr.
//! Nothing is mocked (real process, real filesystem, real SQLite).
//!
//! The contract under test: bare `dirsql` is a REPL. Reading its stdin from a
//! pipe runs one statement per line with no prompt and no banner, a failing
//! statement is reported and the session **continues**, and a clean EOF exits
//! `0` regardless of per-statement failures. The interactive half (prompt,
//! banner, TTY detection) needs a PTY and is covered by the python e2e suite.
//!
//! Gated behind `--features cli`: the `dirsql` bin target itself is
//! `required-features = ["cli"]`, so without the feature there is no binary
//! for `assert_cmd::cargo_bin` to find.

#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Output, Stdio};

use assert_cmd::prelude::*;
use tempfile::TempDir;

/// Run bare `dirsql` (plus any extra argv) over a fresh empty directory,
/// feeding `stdin` through a pipe and closing it to signal EOF.
fn repl(stdin: &str, args: &[&str]) -> (TempDir, Output) {
    let root = TempDir::new().unwrap();
    let mut child = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning `dirsql` failed");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(stdin.as_bytes())
        .expect("writing to the REPL's stdin failed");
    let out = child
        .wait_with_output()
        .expect("waiting on `dirsql` failed");
    (root, out)
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_piped_statement_runs_and_exits_zero() {
    // The headline behavior change: `echo "SELECT 1" | dirsql` used to be an
    // exit-2 usage error and now runs the statement.
    let (_root, out) = repl("SELECT 1 AS n\n", &[]);

    assert_eq!(
        out.status.code(),
        Some(0),
        "a clean session exits 0, got {out:?}"
    );
    assert_eq!(stdout_of(&out).trim(), r#"[{"n":1}]"#);
}

#[test]
fn every_statement_prints_its_own_result() {
    // A REPL is a loop, not a one-shot: each line is executed and rendered in
    // turn rather than the first line winning and the rest being dropped.
    let (_root, out) = repl("SELECT 1 AS n\nSELECT 2 AS n\n", &[]);

    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
    assert_eq!(lines, vec![r#"[{"n":1}]"#, r#"[{"n":2}]"#], "{out:?}");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_failing_statement_does_not_end_the_session() {
    // The one real behavioral difference from `dirsql query`, which exits 1 on
    // the first failure. A REPL that dies on a typo is unusable.
    let (_root, out) = repl("SELECT nope FROM nowhere\nSELECT 1 AS n\n", &[]);

    assert!(
        stderr_of(&out).contains("nowhere"),
        "the failure is named on stderr, got {out:?}"
    );
    assert_eq!(
        stdout_of(&out).trim(),
        r#"[{"n":1}]"#,
        "the statement after the failure still runs, got {out:?}"
    );
}

#[test]
fn a_clean_eof_exits_zero_even_after_a_failure() {
    // Matches interactive `sqlite3`: per-statement failures are reported as
    // they happen and do not colour the session's exit code.
    let (_root, out) = repl("SELECT nope FROM nowhere\n", &[]);

    assert_eq!(
        out.status.code(),
        Some(0),
        "EOF after a failed statement still exits 0, got {out:?}"
    );
}

#[test]
fn blank_lines_produce_no_output() {
    // Pressing enter at a prompt must not emit an empty result or an error.
    let (_root, out) = repl("\n   \n\t\nSELECT 1 AS n\n", &[]);

    assert_eq!(stdout_of(&out).trim(), r#"[{"n":1}]"#, "{out:?}");
    assert_eq!(stderr_of(&out), "", "{out:?}");
}

#[test]
fn exit_stops_reading_the_rest_of_stdin() {
    // `exit` is a REPL word, not SQL: it terminates the session rather than
    // being handed to SQLite as a statement.
    let (_root, out) = repl("SELECT 1 AS n\nexit\nSELECT 2 AS n\n", &[]);

    assert_eq!(stdout_of(&out).trim(), r#"[{"n":1}]"#, "{out:?}");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn quit_stops_reading_the_rest_of_stdin() {
    let (_root, out) = repl("SELECT 1 AS n\nquit\nSELECT 2 AS n\n", &[]);

    assert_eq!(stdout_of(&out).trim(), r#"[{"n":1}]"#, "{out:?}");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn the_piped_path_prints_no_prompt_and_no_banner() {
    // The whole point of the TTY/pipe split: `dirsql < script.sql > out` must
    // yield results alone, with no interactive furniture interleaved.
    let (_root, out) = repl("SELECT 1 AS n\n", &[]);

    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("dirsql>"),
        "no prompt in a pipe, got {out:?}"
    );
    assert!(
        !stdout.contains("Ctrl-D"),
        "no banner in a pipe, got {out:?}"
    );
}

#[test]
fn an_unusable_config_fails_once_instead_of_looping() {
    // `AppState::Unavailable` fails identically on every statement, so
    // readiness is checked once before the loop rather than reported per line.
    let (_root, out) = repl("SELECT 1 AS n\nSELECT 2 AS n\n", &["-c", "missing.toml"]);

    assert_eq!(
        out.status.code(),
        Some(1),
        "a bad config exits 1 without entering the loop, got {out:?}"
    );
    assert_eq!(
        stderr_of(&out)
            .lines()
            .filter(|l| l.contains("missing.toml"))
            .count(),
        1,
        "the config failure is reported exactly once, got {out:?}"
    );
}
