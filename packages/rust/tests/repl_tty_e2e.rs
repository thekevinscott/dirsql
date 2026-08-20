//! End-to-end tests for the interactive (TTY) half of bare `dirsql` (#988).
//!
//! The interactive path only exists when stdin is a terminal, so these drive
//! the real binary under a **real PTY** (util-linux `script`) and type real
//! keystrokes at it, including arrow keys and `Ctrl+C`. Nothing is mocked:
//! real pty, real editor, real process, real filesystem, real SQLite.
//!
//! A pty alone is not enough. The line editor asks the terminal where the
//! cursor is (a `ESC [ 6 n` Device Status Report) and stalls when nothing
//! answers -- there is a pty here but no terminal emulator behind it. So the
//! harness answers that one query itself, which is the whole of the emulation
//! these tests need.
//!
//! What is under test is the part of #988's contract that a byte stream can
//! answer: a statement ends at its semicolon (SQLite's tokenizer decides
//! where), a continuation prompt marks an unfinished one, and the session
//! survives a failure. The **piped** path is deliberately untouched and stays
//! one statement per line -- `repl_pipe_e2e.rs` pins that.
//!
//! What is *not* here, and why: history recall, `Ctrl+C`, and anything else
//! that turns on a keystroke landing exactly when the editor is listening.
//! The editor repaints by moving the cursor, so this harness sees a byte
//! stream rather than a screen and can only guess when a redraw has finished
//! -- measured at roughly one miss in four. Those cases live in the
//! curtaincall suite under `packages/python/tests/e2e/`, which drives a real
//! VT100 emulator and waits on the screen itself.
//!
//! Linux-gated: `script -qec` is the util-linux spelling, and the Rust CI test
//! job runs on Linux.

#![cfg(all(feature = "cli", target_os = "linux"))]

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

/// The cursor-position query the editor sends, and the reply the harness
/// gives: row 1, column 1 is a perfectly good answer for a fresh screen.
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CURSOR_REPLY: &[u8] = b"\x1b[1;1R";

/// Long enough for a debug-build startup and a directory scan, short enough
/// that a wedged editor fails the test instead of hanging the CI job.
const PATIENCE: Duration = Duration::from_secs(30);

/// What the terminal received, with escape sequences removed so assertions
/// read against the visible text.
struct Session {
    screen: String,
    code: Option<i32>,
}

impl Session {
    fn shows(&self, needle: &str) -> bool {
        self.screen.contains(needle)
    }
}

/// A REPL session under a pty, with its own home so the suite never writes to
/// the developer's real history file.
struct Terminal {
    home: TempDir,
    root: TempDir,
}

impl Terminal {
    fn new() -> Self {
        Self {
            home: TempDir::new().unwrap(),
            root: TempDir::new().unwrap(),
        }
    }

    /// Type `keys` at a fresh session and return what the terminal showed.
    ///
    /// Keys are sent only once the banner has been drawn, so the editor is
    /// listening before the first keystroke arrives.
    fn type_in(&self, keys: &str) -> Session {
        let mut child = Command::new("script")
            .arg("-qec")
            .arg(cargo_bin("dirsql").to_str().expect("a UTF-8 binary path"))
            .arg("/dev/null")
            .current_dir(self.root.path())
            // Keep history inside the test's own tree: a suite that wrote to
            // the developer's real history file would be unusable.
            .env("XDG_DATA_HOME", self.home.path())
            .env("TERM", "xterm")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawning `script` failed -- util-linux must provide it");

        let keyboard = Arc::new(Mutex::new(child.stdin.take().expect("stdin was piped")));
        let screen = Arc::new(Mutex::new(Vec::<u8>::new()));
        let finished = Arc::new(AtomicBool::new(false));
        let reader = read_and_answer(
            child.stdout.take().expect("stdout was piped"),
            Arc::clone(&keyboard),
            Arc::clone(&screen),
            Arc::clone(&finished),
        );

        settle(&screen, &finished, |shown| {
            String::from_utf8_lossy(shown).contains("Ctrl-D")
        });
        // One line at a time, waiting for the editor to finish reacting to
        // each. Sent in one burst, a keystroke aimed at the *next* prompt can
        // arrive while the previous statement is still running and be lost --
        // which is exactly what an arrow key looks like when it goes missing.
        for line in keystrokes(keys) {
            let before = screen.lock().unwrap().len();
            if keyboard.lock().unwrap().write_all(line.as_bytes()).is_err() {
                // The session ended on an earlier key (Ctrl-D, `quit`); the
                // rest of the script has nothing to type at.
                break;
            }
            settle(&screen, &finished, |shown| shown.len() > before);
            quiesce(&screen, &finished);
        }

        let code = wait_or_kill(&mut child);
        finished.store(true, Ordering::SeqCst);
        reader.join().expect("the reader thread panicked");

        let mut raw = screen.lock().unwrap().clone();
        let mut errors = Vec::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_end(&mut errors);
        }
        raw.extend_from_slice(&errors);

        Session {
            screen: strip_escapes(&String::from_utf8_lossy(&raw)).replace('\r', ""),
            code,
        }
    }
}

/// Drain the child's output into `screen`, answering each cursor-position
/// query as it arrives. Without the answer the editor never paints.
fn read_and_answer(
    mut stdout: std::process::ChildStdout,
    keyboard: Arc<Mutex<ChildStdin>>,
    screen: Arc<Mutex<Vec<u8>>>,
    finished: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let seen = &chunk[..read];
                    screen.lock().unwrap().extend_from_slice(seen);
                    for _ in 0..count_queries(seen) {
                        if finished.load(Ordering::SeqCst) {
                            break;
                        }
                        let mut keys = keyboard.lock().unwrap();
                        if keys.write_all(CURSOR_REPLY).is_err() {
                            break;
                        }
                        let _ = keys.flush();
                    }
                }
            }
        }
    })
}

fn count_queries(bytes: &[u8]) -> usize {
    bytes
        .windows(CURSOR_QUERY.len())
        .filter(|window| *window == CURSOR_QUERY)
        .count()
}

/// Split typed input into the groups that each end one editor interaction:
/// a line of text, or a control key that returns from `read_line` on its own.
/// Anything sent after such a key in the same burst is still sitting in the
/// pty when the editor restarts, and is lost.
fn keystrokes(keys: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut group = String::new();
    for ch in keys.chars() {
        group.push(ch);
        if ch == '\n' || ch == '\u{3}' || ch == '\u{4}' {
            groups.push(std::mem::take(&mut group));
        }
    }
    if !group.is_empty() {
        groups.push(group);
    }
    groups
}

/// Block until `ready` holds of what the terminal has shown so far, or the
/// session ends, or patience runs out.
fn settle<F>(screen: &Arc<Mutex<Vec<u8>>>, finished: &Arc<AtomicBool>, ready: F)
where
    F: Fn(&[u8]) -> bool,
{
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline && !finished.load(Ordering::SeqCst) {
        if ready(&screen.lock().unwrap()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Block until the terminal stops changing, so the editor has finished
/// reacting to the last keystroke.
///
/// This is a settling heuristic, not a screen read: the editor repaints by
/// moving the cursor, so the byte stream these tests see is not the screen
/// and cannot be turned back into one by dropping escapes. That is why the
/// assertions below ask only whether some text was ever drawn, and why the
/// tests that depend on precise keystroke sequencing (history recall, Ctrl+C)
/// live in the curtaincall suite, which drives a real VT100 emulator.
fn quiesce(screen: &Arc<Mutex<Vec<u8>>>, finished: &Arc<AtomicBool>) {
    let deadline = Instant::now() + PATIENCE;
    let mut last = usize::MAX;
    while Instant::now() < deadline && !finished.load(Ordering::SeqCst) {
        let now = screen.lock().unwrap().len();
        if now == last {
            return;
        }
        last = now;
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// Wait for the session to end, killing it if it wedges so the test reports a
/// failure rather than hanging the job.
fn wait_or_kill(child: &mut Child) -> Option<i32> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        match child.try_wait().expect("waiting on `script` failed") {
            Some(status) => return status.code(),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Drop the escape sequences the editor emits to move the cursor, colour the
/// prompt, and save/restore position.
fn strip_escapes(raw: &str) -> String {
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
            // Two-byte escapes (save/restore cursor, charset selection): the
            // one byte already consumed is the whole of it.
            _ => {}
        }
    }
    out
}

#[test]
fn a_statement_split_across_lines_runs_as_one() {
    // The headline of #988: a statement ends at its semicolon, not at the
    // first newline, so it can be laid out over as many lines as it needs.
    let session = Terminal::new().type_in("SELECT\n1 AS n;\nquit\n");

    assert!(
        session.shows(r#"[{"n":1}]"#),
        "the two lines ran as one statement, got:\n{}",
        session.screen
    );
    assert!(
        !session.shows("incomplete input"),
        "the first line must not be executed on its own, got:\n{}",
        session.screen
    );
}

#[test]
fn an_unfinished_statement_shows_a_continuation_prompt() {
    // Without a distinct prompt the user cannot tell that enter will not run
    // what they have typed.
    let session = Terminal::new().type_in("SELECT\n1 AS n;\nquit\n");

    assert!(
        session.shows("...>"),
        "the second line was asked for, got:\n{}",
        session.screen
    );
}

#[test]
fn a_lone_semicolon_terminates_the_statement_above_it() {
    // The terminator does not have to share a line with the statement.
    let session = Terminal::new().type_in("SELECT 1 AS n\n;\nquit\n");

    assert!(session.shows(r#"[{"n":1}]"#), "got:\n{}", session.screen);
    assert!(
        !session.shows("error"),
        "nothing was executed prematurely, got:\n{}",
        session.screen
    );
}

#[test]
fn a_semicolon_inside_a_string_literal_does_not_terminate() {
    // The classic trap, and the reason completeness is SQLite's tokenizer's
    // call rather than a search for a trailing `;`.
    let session = Terminal::new().type_in("SELECT\n';' AS s;\nquit\n");

    assert!(
        session.shows(r#"[{"s":";"}]"#),
        "the literal's semicolon is data, not a terminator, got:\n{}",
        session.screen
    );
}

#[test]
fn exit_words_still_leave_without_a_terminator() {
    // `exit` is a REPL word, not SQL, so the completeness rule must let it
    // through rather than waiting for a semicolon that never comes.
    let session = Terminal::new().type_in("exit\n");

    assert_eq!(session.code, Some(0), "got:\n{}", session.screen);
    assert!(
        !session.shows("error"),
        "`exit` is not handed to SQLite, got:\n{}",
        session.screen
    );
}

#[test]
fn ctrl_d_leaves_the_session() {
    // The editor reports `Ctrl+D` distinctly from every other signal, and
    // confusing the two would turn the standard way out of a shell into a
    // keystroke that clears the line and does nothing else.
    let session = Terminal::new().type_in("SELECT 1 AS n;\n\u{4}");

    assert!(
        session.shows(r#"[{"n":1}]"#),
        "the statement ran first, got:\n{}",
        session.screen
    );
    assert_eq!(
        session.code,
        Some(0),
        "Ctrl-D ends the session cleanly, got:\n{}",
        session.screen
    );
}

#[test]
fn a_failed_statement_still_leaves_the_session_running() {
    // Slice 1's continue-on-error survives multi-line entry.
    let session = Terminal::new().type_in("SELECT nope\nFROM missing;\nSELECT 1 AS n;\nquit\n");

    assert!(
        session.shows("missing"),
        "the failure names the table, got:\n{}",
        session.screen
    );
    assert!(
        session.shows(r#"[{"n":1}]"#),
        "the next statement still runs, got:\n{}",
        session.screen
    );
    assert_eq!(session.code, Some(0));
}
