//! The read-eval-print loop behind bare `dirsql`.
//!
//! `dirsql` with no subcommand and no SQL reads statements until EOF instead
//! of printing a usage error. The loop is a thin wrapper over the same
//! [`execute_query`] pipeline `dirsql query` and `POST /query` use, so a
//! statement typed at the prompt and one passed on the command line cannot
//! drift.
//!
//! Two halves, selected by one boolean the caller derives from
//! `stdin().is_terminal()`:
//!
//! - **interactive** — a banner, a prompt before each read, and a trailing
//!   newline on exit so the shell prompt starts on its own line.
//! - **piped** — none of that furniture, so `dirsql < script.sql > out.json`
//!   yields results alone.
//!
//! Both halves share the loop, and the differences from one-shot
//! [`run_query`](super::run) are deliberate: a failing statement is reported
//! and the session **continues**, and a clean EOF exits `0` regardless of
//! per-statement failures (matching interactive `sqlite3`).

use std::io::{BufRead, Write};

use serde_json::Value;

use super::AppState;
use super::execute::{QueryFailure, execute_query};
use super::run::query_body;

/// Printed before each read on the interactive path.
const PROMPT: &str = "dirsql> ";

/// Where the REPL sends a statement. The real implementation is the shared
/// [`execute_query`] pipeline; the loop is generic over this so its control
/// flow — continue-on-error, exit words, blank lines — is unit-testable
/// without an index behind it.
trait Statements {
    async fn run(&self, sql: &str) -> Result<Value, QueryFailure>;
}

/// [`Statements`] backed by the real index.
struct StateStatements<'a>(&'a AppState);

impl Statements for StateStatements<'_> {
    async fn run(&self, sql: &str) -> Result<Value, QueryFailure> {
        // Unbounded, exactly as the one-shot CLI is: only the long-lived
        // server enforces `query_timeout`.
        execute_query(self.0, query_body(sql), None).await
    }
}

/// Run the REPL over `input`, rendering results to `out` and diagnostics to
/// `err`, and return the process exit code.
///
/// A degraded index fails identically for every statement, so readiness is
/// checked **once** here and reported once, rather than printing the same
/// line for every statement the user types.
pub(super) async fn run_repl<R, W, E>(
    state: &AppState,
    input: R,
    out: &mut W,
    err: &mut E,
    interactive: bool,
) -> u8
where
    R: BufRead + Send + 'static,
    W: Write,
    E: Write,
{
    if let AppState::Unavailable(reason) = state {
        let _ = writeln!(err, "dirsql: {reason}");
        return 1;
    }
    repl_loop(&StateStatements(state), input, out, err, interactive).await
}

/// The loop itself, generic over its statement sink so unit tests can drive
/// it with a double.
async fn repl_loop<S, R, W, E>(
    statements: &S,
    mut input: R,
    out: &mut W,
    err: &mut E,
    interactive: bool,
) -> u8
where
    S: Statements,
    R: BufRead + Send + 'static,
    W: Write,
    E: Write,
{
    if interactive && write!(out, "{}", banner()).is_err() {
        return 1;
    }
    loop {
        if interactive && (write!(out, "{PROMPT}").is_err() || out.flush().is_err()) {
            return 1;
        }
        let line = match next_line(input).await {
            Ok((reader, Some(line))) => {
                input = reader;
                line
            }
            Ok((_, None)) => break,
            Err(read_err) => {
                let _ = writeln!(err, "dirsql: failed to read input: {read_err}");
                return 1;
            }
        };

        let sql = line.trim();
        if sql.is_empty() {
            continue;
        }
        if is_exit_word(sql) {
            break;
        }

        match statements.run(sql).await {
            Ok(value) => {
                // A closed stdout (`dirsql < script.sql | head -1`) means
                // nothing downstream is listening; keep reading and there is
                // a whole script left to execute into a broken pipe.
                if writeln!(out, "{value}").is_err() {
                    return 0;
                }
            }
            Err(failure) => {
                let _ = writeln!(err, "dirsql: {}", failure.message());
            }
        }
    }
    if interactive {
        let _ = writeln!(out);
    }
    0
}

/// Read one line off `reader`, handing the reader back. `None` is EOF.
///
/// The read happens on a blocking thread: `run_cli` drives the whole CLI
/// inside `runtime.block_on`, so reading inline would park the thread the
/// runtime is driven on while the live watcher still needs it.
async fn next_line<R>(mut reader: R) -> std::io::Result<(R, Option<String>)>
where
    R: BufRead + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        // A count of 0 is EOF and only EOF: a real line always carries at
        // least its terminator, so an empty `line` cannot mean anything else.
        match reader.read_line(&mut line)? {
            0 => Ok((reader, None)),
            _ => Ok((reader, Some(line))),
        }
    })
    .await
    .map_err(std::io::Error::other)?
}

/// `exit` and `quit` end the session. Neither is a valid SQL statement, so
/// there is nothing to namespace them against — which is why dirsql has no
/// dot-commands (#953).
fn is_exit_word(line: &str) -> bool {
    line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit")
}

/// The interactive greeting. Static text only: no scan and no live rows, so
/// the first prompt appears immediately even in a large tree (#986).
fn banner() -> String {
    format!(
        "dirsql {version} — this directory is a database.\n\
         \n  \
         SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5\n  \
         SELECT path FROM './**/*.md' WHERE content LIKE '%TODO%'\n\
         \n\
         `exit`, `quit`, or Ctrl-D to leave.\n\n",
        version = env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Cursor;

    /// A [`Statements`] double: records every statement it is handed and
    /// replies from a scripted queue, so the loop is exercised with no index.
    #[derive(Default)]
    struct FakeStatements {
        seen: RefCell<Vec<String>>,
        replies: RefCell<Vec<Result<Value, QueryFailure>>>,
    }

    impl FakeStatements {
        fn failing(message: &str) -> Self {
            let fake = Self::default();
            fake.replies
                .borrow_mut()
                .push(Err(QueryFailure::BadRequest(message.to_string())));
            fake
        }

        fn seen(&self) -> Vec<String> {
            self.seen.borrow().clone()
        }
    }

    impl Statements for FakeStatements {
        async fn run(&self, sql: &str) -> Result<Value, QueryFailure> {
            self.seen.borrow_mut().push(sql.to_string());
            self.replies
                .borrow_mut()
                .pop()
                .unwrap_or_else(|| Ok(Value::Array(vec![])))
        }
    }

    /// A sink whose every write fails, standing in for a closed pipe.
    struct BrokenPipe;

    impl Write for BrokenPipe {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken pipe"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("broken pipe"))
        }
    }

    /// A sink that accepts writes but cannot flush -- a prompt sitting in a
    /// buffer whose drain has gone away.
    struct FlushFails;

    impl Write for FlushFails {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("cannot flush"))
        }
    }

    /// A reader that fails instead of yielding a line, standing in for a
    /// mid-session I/O error on stdin.
    struct BrokenReader;

    impl std::io::Read for BrokenReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("input went away"))
        }
    }

    impl BufRead for BrokenReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Err(std::io::Error::other("input went away"))
        }

        fn consume(&mut self, _amt: usize) {}
    }

    fn input(script: &str) -> Cursor<Vec<u8>> {
        Cursor::new(script.as_bytes().to_vec())
    }

    fn text(sink: &[u8]) -> String {
        String::from_utf8(sink.to_vec()).expect("output must be UTF-8")
    }

    async fn drive(script: &str, interactive: bool) -> (FakeStatements, String, String, u8) {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = repl_loop(&fake, input(script), &mut out, &mut err, interactive).await;
        (fake, text(&out), text(&err), code)
    }

    #[tokio::test]
    async fn every_non_blank_line_is_executed_in_order() {
        let (fake, _, _, code) = drive("SELECT 1\nSELECT 2\n", false).await;

        assert_eq!(fake.seen(), vec!["SELECT 1", "SELECT 2"]);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn each_result_is_written_on_its_own_line() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(
            &fake,
            input("SELECT 1\nSELECT 2\n"),
            &mut out,
            &mut err,
            false,
        )
        .await;

        assert_eq!(text(&out), "[]\n[]\n");
    }

    #[tokio::test]
    async fn statements_are_trimmed_before_execution() {
        let (fake, _, _, _) = drive("   SELECT 1   \n", false).await;

        assert_eq!(fake.seen(), vec!["SELECT 1"]);
    }

    #[tokio::test]
    async fn blank_and_whitespace_lines_are_skipped() {
        let (fake, out, err, code) = drive("\n   \n\t\n", false).await;

        assert!(fake.seen().is_empty(), "nothing was executed");
        assert_eq!(out, "", "a blank line renders nothing");
        assert_eq!(err, "", "a blank line is not an error");
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn a_final_line_without_a_newline_still_runs() {
        // The last line of a heredoc or `printf` often arrives unterminated.
        let (fake, _, _, _) = drive("SELECT 1", false).await;

        assert_eq!(fake.seen(), vec!["SELECT 1"]);
    }

    #[tokio::test]
    async fn exit_ends_the_session_before_the_rest_of_the_input() {
        let (fake, _, _, code) = drive("SELECT 1\nexit\nSELECT 2\n", false).await;

        assert_eq!(fake.seen(), vec!["SELECT 1"]);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn quit_ends_the_session_before_the_rest_of_the_input() {
        let (fake, _, _, code) = drive("quit\nSELECT 2\n", false).await;

        assert!(fake.seen().is_empty());
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn a_failure_is_reported_and_the_session_continues() {
        // The one real behavioral difference from `dirsql query`, which exits
        // 1 on the first failure.
        let fake = FakeStatements::failing("no such table: nowhere");
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = repl_loop(
            &fake,
            input("SELECT nope\nSELECT 1\n"),
            &mut out,
            &mut err,
            false,
        )
        .await;

        assert_eq!(text(&err), "dirsql: no such table: nowhere\n");
        assert_eq!(fake.seen(), vec!["SELECT nope", "SELECT 1"]);
        assert_eq!(code, 0, "a clean EOF exits 0 even after a failure");
    }

    #[tokio::test]
    async fn a_failure_renders_nothing_on_the_result_sink() {
        let fake = FakeStatements::failing("boom");
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(&fake, input("SELECT nope\n"), &mut out, &mut err, false).await;

        assert_eq!(text(&out), "", "errors never reach the result sink");
    }

    #[tokio::test]
    async fn the_piped_path_writes_no_prompt_and_no_banner() {
        let (_, out, _, _) = drive("SELECT 1\n", false).await;

        assert_eq!(out, "[]\n", "results alone, no interactive furniture");
    }

    #[tokio::test]
    async fn the_interactive_path_opens_with_the_banner() {
        let (_, out, _, _) = drive("", true).await;

        assert!(
            out.starts_with(&banner()),
            "banner comes first, got {out:?}"
        );
    }

    #[tokio::test]
    async fn the_interactive_path_prompts_before_every_read() {
        let (_, out, _, _) = drive("SELECT 1\nSELECT 2\n", true).await;

        // Two statements plus the read that hits EOF.
        assert_eq!(out.matches(PROMPT).count(), 3, "got {out:?}");
    }

    #[tokio::test]
    async fn the_interactive_path_closes_with_a_newline() {
        // Ctrl-D leaves the cursor mid-line; without this the shell prompt
        // returns on top of the dirsql prompt.
        let (_, out, _, _) = drive("", true).await;

        assert!(out.ends_with("\n"), "got {out:?}");
    }

    #[tokio::test]
    async fn a_read_failure_ends_the_session_with_an_error() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = repl_loop(&fake, BrokenReader, &mut out, &mut err, false).await;

        assert_eq!(code, 1);
        assert_eq!(
            text(&err),
            "dirsql: failed to read input: input went away\n"
        );
    }

    #[tokio::test]
    async fn a_closed_result_sink_stops_the_loop_cleanly() {
        // `dirsql < script.sql | head -1`: nothing downstream is listening, so
        // executing the rest of the script is wasted work.
        let fake = FakeStatements::default();
        let mut err = Vec::new();

        let code = repl_loop(
            &fake,
            input("SELECT 1\nSELECT 2\n"),
            &mut BrokenPipe,
            &mut err,
            false,
        )
        .await;

        assert_eq!(code, 0);
        assert_eq!(fake.seen(), vec!["SELECT 1"], "the second never runs");
    }

    #[tokio::test]
    async fn a_closed_sink_abandons_the_banner_rather_than_looping() {
        let fake = FakeStatements::default();
        let mut err = Vec::new();

        let code = repl_loop(&fake, input("SELECT 1\n"), &mut BrokenPipe, &mut err, true).await;

        assert_eq!(code, 1);
        assert!(fake.seen().is_empty());
    }

    #[tokio::test]
    async fn an_unflushable_prompt_stops_the_loop_too() {
        // A prompt that reaches the buffer but never the terminal leaves the
        // user typing blind, so it is as fatal as one that cannot be written
        // at all -- both halves of the prompt write have to be checked.
        let fake = FakeStatements::default();
        let mut err = Vec::new();

        let code = repl_loop(&fake, input("SELECT 1\n"), &mut FlushFails, &mut err, true).await;

        assert_eq!(code, 1);
        assert!(fake.seen().is_empty(), "nothing runs behind a dead prompt");
    }

    #[tokio::test]
    async fn a_degraded_index_is_reported_once_and_never_loops() {
        // `AppState::Unavailable` fails identically for every statement, so
        // repeating it per line is noise rather than information.
        let state = AppState::Unavailable("failed to resolve missing.toml".to_string());
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = run_repl(
            &state,
            input("SELECT 1\nSELECT 2\n"),
            &mut out,
            &mut err,
            false,
        )
        .await;

        assert_eq!(code, 1);
        assert_eq!(text(&err), "dirsql: failed to resolve missing.toml\n");
        assert_eq!(text(&out), "");
    }

    #[tokio::test]
    async fn the_real_statement_sink_answers_from_the_shared_pipeline() {
        // `StateStatements` is the loop's only route to `execute_query`. One
        // that answered without consulting the index would make every result
        // the REPL prints fiction, so pin that it reports the index's state.
        let state = AppState::Unavailable("failed to load config".to_string());

        let failure = StateStatements(&state)
            .run("SELECT 1")
            .await
            .expect_err("a degraded index cannot answer a statement");

        assert_eq!(failure.message(), "failed to load config");
    }

    #[test]
    fn exit_words_are_recognized_regardless_of_case() {
        for word in ["exit", "quit", "EXIT", "Quit", "eXiT"] {
            assert!(is_exit_word(word), "{word} ends the session");
        }
    }

    #[test]
    fn nothing_else_is_an_exit_word() {
        // A prefix match would swallow `SELECT quitters` and `exit_code`.
        for word in ["exits", "quitting", "exit;", ".exit", "SELECT 1", ""] {
            assert!(!is_exit_word(word), "{word} is not an exit word");
        }
    }

    #[test]
    fn the_banner_names_the_version_and_how_to_leave() {
        let banner = banner();

        assert!(banner.contains(env!("CARGO_PKG_VERSION")), "{banner}");
        assert!(banner.contains("Ctrl-D"), "{banner}");
        assert!(banner.contains("exit"), "{banner}");
    }

    #[test]
    fn the_banner_shows_runnable_sample_queries() {
        // The greeting is the whole onboarding surface: a user who has never
        // seen a path-table needs one here.
        let banner = banner();

        assert!(
            banner.contains("SELECT basename, size FROM './'"),
            "{banner}"
        );
        assert!(banner.contains("FROM './**/*.md'"), "{banner}");
    }

    #[test]
    fn the_banner_ends_with_a_blank_line_before_the_prompt() {
        assert!(banner().ends_with("\n\n"), "{}", banner());
    }
}
