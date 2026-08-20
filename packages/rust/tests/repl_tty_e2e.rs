//! End-to-end tests for the interactive (TTY) half of bare `dirsql` (#988).
//!
//! The interactive path only exists when stdin is a terminal, so these drive
//! the real binary under a **real PTY** allocated by util-linux `script`,
//! feeding keystrokes and reading back what the terminal received. Nothing is
//! mocked: real pty, real process, real filesystem, real SQLite.
//!
//! The contract under test is multi-line entry: a statement ends at its
//! semicolon, as decided by SQLite's own tokenizer, so a statement may span
//! as many lines as it needs and a semicolon inside a string literal does not
//! end it. The **piped** path is deliberately untouched by this and stays one
//! statement per line -- `repl_pipe_e2e.rs` pins that.
//!
//! Assertions are on the terminal's final contents rather than on timing, so
//! feeding stdin from a pipe is safe: the pty buffers the keystrokes and the
//! editor consumes them in order. Behaviors that genuinely depend on
//! keystroke *timing* (arrow-key recall, Ctrl+C mid-line) are covered by the
//! curtaincall suite in `packages/python/tests/e2e/`, which auto-waits.
//!
//! Linux-gated: `script -qec` is the util-linux spelling, and the Rust CI test
//! job runs on Linux.

#![cfg(all(feature = "cli", target_os = "linux"))]

use std::io::Write;
use std::process::{Command, Stdio};

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

/// What the terminal received, with ANSI escape sequences removed so
/// assertions read against the visible text the editor drew.
struct Session {
    screen: String,
    code: Option<i32>,
}

/// Run the real binary under a PTY over a fresh directory holding `files`,
/// typing `keys`.
fn repl(keys: &str, files: &[(&str, &str)]) -> Session {
    let root = TempDir::new().unwrap();
    for (name, body) in files {
        std::fs::write(root.path().join(name), body).unwrap();
    }
    let binary = cargo_bin("dirsql");

    let mut child = Command::new("script")
        .arg("-qec")
        .arg(binary.to_str().expect("the binary path must be UTF-8"))
        .arg("/dev/null")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning `script` failed -- util-linux must provide it");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(keys.as_bytes())
        .expect("typing at the pty failed");
    let out = child
        .wait_with_output()
        .expect("waiting on `script` failed");

    // `script` merges the child's stdout and stderr onto the pty, which is
    // what a user sees; the split is asserted by the piped suite instead.
    let mut screen = String::from_utf8_lossy(&out.stdout).into_owned();
    screen.push_str(&String::from_utf8_lossy(&out.stderr));
    Session {
        screen: strip_ansi(&screen).replace('\r', ""),
        code: out.status.code(),
    }
}

/// Drop CSI / OSC escape sequences the line editor emits to move the cursor
/// and colour the prompt.
fn strip_ansi(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then a byte in `@`..`~` ends it.
            Some('[') => {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: ends at BEL or ST (`ESC \`).
            Some(']') => {
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn a_statement_split_across_lines_runs_as_one() {
    // The headline of #988: a statement ends at its semicolon, not at the
    // first newline, so it can be laid out over as many lines as it needs.
    let session = repl("SELECT\n1 AS n;\nquit\n", &[]);

    assert!(
        session.screen.contains(r#"[{"n":1}]"#),
        "the two lines ran as one statement, got:\n{}",
        session.screen
    );
    assert!(
        !session.screen.contains("incomplete input"),
        "the first line must not be executed on its own, got:\n{}",
        session.screen
    );
}

#[test]
fn a_lone_semicolon_terminates_the_statement_above_it() {
    // The terminator does not have to share a line with the statement.
    let session = repl("SELECT 1 AS n\n;\nquit\n", &[]);

    assert!(
        session.screen.contains(r#"[{"n":1}]"#),
        "got:\n{}",
        session.screen
    );
    assert!(
        !session.screen.contains("error"),
        "nothing was executed prematurely, got:\n{}",
        session.screen
    );
}

#[test]
fn a_semicolon_inside_a_string_literal_does_not_terminate() {
    // The classic trap, and the reason completeness is SQLite's tokenizer's
    // call rather than a search for a trailing `;`.
    let session = repl("SELECT\n';' AS s;\nquit\n", &[]);

    assert!(
        session.screen.contains(r#"[{"s":";"}]"#),
        "the literal's semicolon is data, not a terminator, got:\n{}",
        session.screen
    );
}

#[test]
fn an_unterminated_statement_is_never_executed() {
    // Reaching EOF mid-statement discards the fragment rather than running
    // half of it.
    let session = repl("SELECT 1 AS n\n", &[]);

    assert!(
        !session.screen.contains(r#"[{"n":1}]"#),
        "a fragment must not run, got:\n{}",
        session.screen
    );
    assert_eq!(session.code, Some(0), "EOF is still a clean exit");
}

#[test]
fn exit_words_still_leave_without_a_terminator() {
    // `exit` is a REPL word, not SQL, so the completeness rule must let it
    // through rather than waiting for a semicolon that never comes.
    let session = repl("exit\n", &[]);

    assert_eq!(session.code, Some(0), "got:\n{}", session.screen);
    assert!(
        !session.screen.contains("error"),
        "`exit` is not handed to SQLite, got:\n{}",
        session.screen
    );
}

#[test]
fn a_completed_statement_still_reports_its_own_failure_and_continues() {
    // Slice 1's continue-on-error survives multi-line entry.
    let session = repl("SELECT nope\nFROM missing;\nSELECT 1 AS n;\nquit\n", &[]);

    assert!(
        session.screen.contains("missing"),
        "the failure names the table, got:\n{}",
        session.screen
    );
    assert!(
        session.screen.contains(r#"[{"n":1}]"#),
        "the next statement still runs, got:\n{}",
        session.screen
    );
    assert_eq!(session.code, Some(0));
}
