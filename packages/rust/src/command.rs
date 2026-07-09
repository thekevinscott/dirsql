//! Reusable shell-command runner — the foundation for the command-backed
//! events (`on-file`, `pre-query`, `post-query`), which are thin wiring on
//! top of it.
//!
//! ## Contract
//!
//! - **argv, not a shell.** The command *template* is split into argv with
//!   shell-like quoting (via [`shlex`]) so `sh -c '…'` keeps its script as a
//!   single argument — but no shell is ever invoked: there is no globbing,
//!   piping, or variable expansion. To get a real shell, ask for one
//!   explicitly (`sh -c '…'`).
//! - **Placeholders.** `{path}`, `{args}`, `{root}` (and any
//!   others a caller supplies) are substituted into whole argv tokens, every
//!   occurrence. Substitution is single-pass and left-to-right per token, so a
//!   value that itself contains `{…}` is never re-scanned — a substituted value
//!   is always exactly one argv element, keeping values with spaces (and
//!   untrusted input) injection-safe. An unknown `{…}` is left literal.
//! - **cwd / env / timeout.** The child runs in `cwd` (the config file's
//!   directory), inherits dirsql's environment (so `uvx --with …` / `npx …`
//!   dependency resolution works), and is killed if it exceeds `timeout`.
//! - **stdin.** An optional payload is written to the child's stdin (used by
//!   events whose payload may exceed the OS argv limit).
//! - **Framing.** The output payload is the **last non-empty line of stdout**;
//!   any chatter/log lines above it are ignored. stderr is never data — it is
//!   captured only to enrich errors.
//! - **Errors.** A non-zero exit or a timeout is a failure carrying the tail of
//!   stderr.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// The default timeout for every command-backed event (`on-file`,
/// `pre-query`, `post-query`) when the config declares no override
/// (`[dirsql].hook-timeout`, positive whole seconds).
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// A named placeholder substituted into a command's argv.
///
/// `name` is the bare identifier (no braces): a `name` of `path` matches the
/// template token `{path}`. Substitution only — a placeholder whose `{name}`
/// never appears in the template is a no-op.
#[derive(Debug, Clone)]
pub struct Placeholder {
    pub name: String,
    pub value: String,
}

impl Placeholder {
    /// Replaces `{name}` where it appears, and is a no-op when the template
    /// omits it.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// A successful command run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The last non-empty line of stdout (trimmed).
    pub payload: String,
}

/// Everything that can go wrong running a command.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// The template was empty (or only whitespace), or its quoting was
    /// unbalanced, so it produced no argv.
    #[error("invalid command template: {0:?}")]
    InvalidCommand(String),

    /// The child could not be spawned (e.g. the program was not found).
    #[error("failed to spawn `{command}`: {source}")]
    Spawn {
        command: String,
        #[source]
        source: std::io::Error,
    },

    /// The child exited with a non-zero status. `code` is the exit code, or
    /// `"signal"` when the child was terminated by a signal.
    #[error("command `{command}` failed (exit {code}): {stderr_tail}")]
    NonZeroExit {
        command: String,
        code: String,
        stderr_tail: String,
    },

    /// The child ran longer than the configured timeout and was killed.
    #[error("command `{command}` timed out after {timeout:?}: {stderr_tail}")]
    Timeout {
        command: String,
        timeout: Duration,
        stderr_tail: String,
    },

    /// The child exited cleanly but wrote no non-empty line to stdout.
    #[error("command `{command}` produced no output on stdout")]
    EmptyOutput { command: String },

    /// An I/O error while waiting on the child.
    #[error("command `{command}` I/O error: {source}")]
    Io {
        command: String,
        #[source]
        source: std::io::Error,
    },
}

/// Run `command` to completion and return its payload.
///
/// See the [module docs](self) for the full contract. `cwd` is the child's
/// working directory (the config file's directory), `timeout` bounds the run,
/// and `stdin_payload`, when `Some`, is written to the child's stdin.
pub fn run_command(
    command: &str,
    placeholders: &[Placeholder],
    cwd: &Path,
    timeout: Duration,
    stdin_payload: Option<&[u8]>,
) -> Result<CommandOutput, CommandError> {
    let argv = build_argv(command, placeholders)?;
    // `build_argv` guarantees a non-empty argv.
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin_payload.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });

    let mut child = cmd.spawn().map_err(|source| CommandError::Spawn {
        command: command.to_string(),
        source,
    })?;

    // Feed stdin and drain stdout/stderr on their own threads. Draining the
    // output pipes concurrently is required: a child that writes more than a
    // pipe buffer would otherwise block on write while we block on `wait`,
    // deadlocking. Writing stdin on a thread lets a large payload flow while
    // the child streams output.
    let stdin_thread = stdin_payload.map(|payload| {
        let mut stdin = child.stdin.take().expect("stdin piped");
        let payload = payload.to_vec();
        std::thread::spawn(move || {
            // A child that ignores stdin closes the pipe early; a broken-pipe
            // write is expected, not an error. Dropping `stdin` sends EOF.
            let _ = stdin.write_all(&payload);
        })
    });

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let status = match child
        .wait_timeout(timeout)
        .map_err(|source| CommandError::Io {
            command: command.to_string(),
            source,
        })? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            if let Some(t) = stdin_thread {
                let _ = t.join();
            }
            let _ = stdout_thread.join();
            let stderr = stderr_thread.join().unwrap_or_default();
            return Err(CommandError::Timeout {
                command: command.to_string(),
                timeout,
                stderr_tail: stderr_tail(&stderr),
            });
        }
    };

    if let Some(t) = stdin_thread {
        let _ = t.join();
    }
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        return Err(CommandError::NonZeroExit {
            command: command.to_string(),
            code,
            stderr_tail: stderr_tail(&stderr),
        });
    }

    let stdout = String::from_utf8_lossy(&stdout);
    match extract_payload(&stdout) {
        Some(payload) => Ok(CommandOutput { payload }),
        None => Err(CommandError::EmptyOutput {
            command: command.to_string(),
        }),
    }
}

/// Split a command template into argv (shell-like quoting, no shell) and
/// substitute placeholders into whole tokens.
fn build_argv(command: &str, placeholders: &[Placeholder]) -> Result<Vec<String>, CommandError> {
    let tokens =
        shlex::split(command).ok_or_else(|| CommandError::InvalidCommand(command.to_string()))?;
    if tokens.is_empty() {
        return Err(CommandError::InvalidCommand(command.to_string()));
    }

    let argv: Vec<String> = tokens
        .iter()
        .map(|token| substitute(token, placeholders))
        .collect();

    Ok(argv)
}

/// Replace every `{name}` in `token` with its placeholder value in a single
/// left-to-right pass. Injected values are never re-scanned, so an untrusted
/// value containing `{…}` is inert. Unknown `{…}` sequences are left literal.
fn substitute(token: &str, placeholders: &[Placeholder]) -> String {
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while i < token.len() {
        if let Some((idx, consumed)) = match_placeholder(token, i, placeholders) {
            out.push_str(&placeholders[idx].value);
            i += consumed;
            continue;
        }
        // Not a recognized placeholder: copy one whole UTF-8 char.
        let ch = token[i..].chars().next().expect("valid utf-8");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// If a recognized `{name}` starts at byte `i` of `token`, return the matching
/// placeholder index and the number of bytes consumed; otherwise `None`.
fn match_placeholder(
    token: &str,
    i: usize,
    placeholders: &[Placeholder],
) -> Option<(usize, usize)> {
    // Braces are ASCII, so byte indexing is safe at these positions.
    if token.as_bytes()[i] != b'{' {
        return None;
    }
    let rel = token[i + 1..].find('}')?;
    let name = &token[i + 1..i + 1 + rel];
    let idx = placeholders.iter().position(|p| p.name == name)?;
    Some((idx, 1 + rel + 1))
}

/// The payload framing: the last line of `stdout` that is non-empty after
/// trimming, returned trimmed. `None` when every line is blank.
fn extract_payload(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// The tail of stderr for error messages: lossy UTF-8, trimmed, capped to the
/// last `MAX` characters (prefixed with `…` when truncated).
fn stderr_tail(stderr: &[u8]) -> String {
    const MAX: usize = 2000;
    let text = String::from_utf8_lossy(stderr);
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= MAX {
        trimmed.to_string()
    } else {
        let tail: String = trimmed.chars().skip(count - MAX).collect();
        format!("…{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(command: &str, placeholders: &[Placeholder]) -> Vec<String> {
        build_argv(command, placeholders).expect("valid command")
    }

    #[test]
    fn splits_on_whitespace_without_placeholders() {
        assert_eq!(argv("echo hello world", &[]), ["echo", "hello", "world"]);
    }

    #[test]
    fn respects_shell_quoting_so_sh_c_keeps_its_script_as_one_arg() {
        assert_eq!(
            argv("sh -c 'echo one two'", &[]),
            ["sh", "-c", "echo one two"]
        );
    }

    #[test]
    fn empty_or_whitespace_template_is_invalid() {
        assert!(matches!(
            build_argv("", &[]),
            Err(CommandError::InvalidCommand(_))
        ));
        assert!(matches!(
            build_argv("   ", &[]),
            Err(CommandError::InvalidCommand(_))
        ));
    }

    #[test]
    fn unbalanced_quotes_are_invalid() {
        assert!(matches!(
            build_argv("sh -c 'unterminated", &[]),
            Err(CommandError::InvalidCommand(_))
        ));
    }

    #[test]
    fn substitutes_a_single_placeholder() {
        assert_eq!(
            argv("cat {path}", &[Placeholder::new("path", "a/b.json")]),
            ["cat", "a/b.json"]
        );
    }

    #[test]
    fn substitutes_all_occurrences_across_and_within_tokens() {
        assert_eq!(
            argv(
                "cp {path} {path}.bak --label={path}",
                &[Placeholder::new("path", "x")]
            ),
            ["cp", "x", "x.bak", "--label=x"]
        );
    }

    #[test]
    fn a_substituted_value_with_spaces_stays_a_single_arg() {
        assert_eq!(
            argv("read {path}", &[Placeholder::new("path", "my file.json")]),
            ["read", "my file.json"]
        );
    }

    #[test]
    fn substitution_is_single_pass_so_injected_braces_are_not_rescanned() {
        assert_eq!(
            argv(
                "run {args} {path}",
                &[
                    Placeholder::new("args", "hello {path}"),
                    Placeholder::new("path", "REAL"),
                ]
            ),
            ["run", "hello {path}", "REAL"]
        );
    }

    #[test]
    fn unknown_placeholder_is_left_literal() {
        assert_eq!(
            argv("echo {unknown} {path}", &[Placeholder::new("path", "p")]),
            ["echo", "{unknown}", "p"]
        );
    }

    #[test]
    fn a_placeholder_the_template_omits_is_dropped() {
        assert_eq!(argv("run", &[Placeholder::new("path", "/tmp/x")]), ["run"]);
    }

    #[test]
    fn extract_payload_takes_the_last_non_empty_line() {
        assert_eq!(
            extract_payload("log line\n[{\"a\":1}]\n"),
            Some("[{\"a\":1}]".to_string())
        );
    }

    #[test]
    fn extract_payload_ignores_leading_chatter_and_trailing_blanks() {
        assert_eq!(
            extract_payload("starting...\ndone\nPAYLOAD\n\n   \n"),
            Some("PAYLOAD".to_string())
        );
    }

    #[test]
    fn extract_payload_is_none_when_all_lines_are_blank() {
        assert_eq!(extract_payload(""), None);
        assert_eq!(extract_payload("\n  \n\t\n"), None);
    }

    #[test]
    fn stderr_tail_passes_short_output_through_trimmed() {
        assert_eq!(stderr_tail(b"\n  boom  \n"), "boom");
    }

    #[test]
    fn stderr_tail_truncates_long_output_to_the_tail() {
        let long = "x".repeat(3000);
        let tail = stderr_tail(long.as_bytes());
        assert_eq!(tail.chars().count(), 2001); // 2000 chars + the leading '…'
        assert!(tail.starts_with('…'));
        assert!(tail.ends_with('x'));
    }

    // The run_command tests spawn real `sh`/`echo`/`cat`. Their test code must
    // statically reference only `super::` items (plus pure std) to satisfy the
    // unit-lint isolation rule.

    fn cwd() -> std::path::PathBuf {
        std::path::PathBuf::from(".")
    }

    #[test]
    fn run_command_returns_last_nonempty_stdout_line() {
        let out = run_command(
            "sh -c 'echo chatter; echo PAYLOAD'",
            &[],
            &cwd(),
            Duration::from_secs(30),
            None,
        )
        .unwrap();
        assert_eq!(out.payload, "PAYLOAD");
    }

    #[test]
    fn run_command_writes_stdin_payload_to_the_child() {
        let out = run_command(
            "cat",
            &[],
            &cwd(),
            Duration::from_secs(30),
            Some(b"hello-stdin"),
        )
        .unwrap();
        assert_eq!(out.payload, "hello-stdin");
    }

    #[test]
    fn run_command_reports_nonzero_exit_with_stderr_tail() {
        let err = run_command(
            "sh -c 'echo oops >&2; exit 3'",
            &[],
            &cwd(),
            Duration::from_secs(30),
            None,
        )
        .unwrap_err();
        match err {
            CommandError::NonZeroExit {
                code, stderr_tail, ..
            } => {
                assert_eq!(code, "3");
                assert!(stderr_tail.contains("oops"), "got: {stderr_tail}");
            }
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn run_command_reports_empty_output_when_stdout_is_blank() {
        let err = run_command("true", &[], &cwd(), Duration::from_secs(30), None).unwrap_err();
        assert!(
            matches!(err, CommandError::EmptyOutput { .. }),
            "got: {err:?}"
        );
    }

    #[test]
    fn run_command_reports_spawn_failure_for_a_missing_program() {
        let err = run_command(
            "definitely-not-a-real-binary-xyzzy",
            &[],
            &cwd(),
            Duration::from_secs(30),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::Spawn { .. }), "got: {err:?}");
    }

    #[test]
    fn run_command_kills_and_reports_timeout_even_with_stdin() {
        let err = run_command(
            "sh -c 'sleep 30'",
            &[],
            &cwd(),
            Duration::from_millis(50),
            Some(b"x"),
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::Timeout { .. }), "got: {err:?}");
    }
}
