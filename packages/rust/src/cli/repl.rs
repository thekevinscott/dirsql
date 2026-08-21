//! The read-eval-print loop behind bare `dirsql`.
//!
//! `dirsql` with no subcommand and no SQL reads statements until EOF instead
//! of printing a usage error. The loop is a thin wrapper over the same
//! [`execute_query`] pipeline `dirsql query` and `POST /query` use, so a
//! statement typed at the prompt and one passed on the command line cannot
//! drift.
//!
//! Two halves, selected by one boolean the caller derives from
//! `stdin().is_terminal()`, differing only in where their statements come
//! from ([`Lines`]):
//!
//! - **interactive** — a banner, then [`reedline`]: history recall and
//!   `Ctrl+R` search, in-line editing, `Ctrl+C` to abandon a line without
//!   killing the process, and multi-line entry. A statement ends at its
//!   semicolon, exactly as in `sqlite3`, and SQLite's own tokenizer decides
//!   where that semicolon is (see [`sql_is_complete`]).
//! - **piped** — a plain line reader, so `dirsql < script.sql > out.json`
//!   yields results alone. **One statement per line, no terminator needed**:
//!   a redirected script is not being typed, so there is no continuation
//!   prompt for a multi-line rule to hang off.
//!
//! Both halves share the loop, and the differences from one-shot
//! [`run_query`](super::run) are deliberate: a failing statement is reported
//! and the session **continues**, and a clean EOF exits `0` regardless of
//! per-statement failures (matching interactive `sqlite3`).

use std::borrow::Cow;
use std::ffi::CString;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use reedline::{
    DefaultPrompt, DefaultPromptSegment, FileBackedHistory, Prompt, PromptEditMode,
    PromptHistorySearch, Reedline, Signal, ValidationResult, Validator,
};
use serde_json::Value;

use super::AppState;
use super::execute::{QueryFailure, execute_query};
use super::run::{Format, query_body, render_rows};

/// The interactive prompt, split as reedline renders it: the left half, then
/// the indicator that follows it.
const PROMPT_LEFT: &str = "dirsql";

/// Shown instead of the indicator while a statement is still being typed.
const PROMPT_CONTINUATION: &str = "   ...> ";

/// How many statements the history file keeps. reedline's own default; a
/// REPL history is recall, not an archive.
const HISTORY_CAPACITY: usize = 1000;

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

/// One read from the user, and what ended it.
enum Entry {
    /// A statement to run. On the interactive path this is a whole statement,
    /// however many lines it took to type.
    Statement(String),
    /// `Ctrl+C`: the line was abandoned. The session continues from a fresh
    /// prompt rather than dying, which is the whole point of routing the
    /// interrupt through the editor instead of a signal handler.
    Cancelled,
    /// `Ctrl+D`, or the end of a redirected script.
    Ended,
}

/// Where the REPL's statements come from. The two implementations are the
/// TTY/pipe split: an editor on one side, a plain line reader on the other.
trait Lines {
    async fn next(&mut self) -> std::io::Result<Entry>;
}

/// Run the REPL over `input`, rendering results to `out` and diagnostics to
/// `err`, and return the process exit code.
///
/// A degraded index fails identically for every statement, so readiness is
/// checked **once** here and reported once, rather than printing the same
/// line for every statement the user types.
pub(super) async fn run_repl<R, W, E>(
    state: &AppState,
    format: Format,
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
    let statements = StateStatements(state);

    if interactive {
        repl_loop(&statements, EditorLines::new(), out, err, format, true).await
    } else {
        repl_loop(
            &statements,
            PipedLines(Some(input)),
            out,
            err,
            format,
            false,
        )
        .await
    }
}

/// The loop itself, generic over both its statement sink and its input, so
/// unit tests can drive it with doubles.
async fn repl_loop<S, L, W, E>(
    statements: &S,
    mut lines: L,
    out: &mut W,
    err: &mut E,
    format: Format,
    interactive: bool,
) -> u8
where
    S: Statements,
    L: Lines,
    W: Write,
    E: Write,
{
    if interactive && (write!(out, "{}", banner()).is_err() || out.flush().is_err()) {
        return 1;
    }
    loop {
        let entry = match lines.next().await {
            Ok(entry) => entry,
            Err(read_err) => {
                let _ = writeln!(err, "dirsql: failed to read input: {read_err}");
                return 1;
            }
        };

        let statement = match entry {
            Entry::Statement(statement) => statement,
            Entry::Cancelled => continue,
            Entry::Ended => break,
        };

        let sql = statement.trim();
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
                if write!(out, "{}", render_rows(&value, format)).is_err() {
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

/// The piped half: one statement per line, straight off the reader.
///
/// The reader lives in an `Option` so it can be moved onto a blocking thread
/// and back for each read. `run_cli` drives the whole CLI inside
/// `runtime.block_on`, so reading inline would park the thread the runtime is
/// driven on while the live watcher still needs it.
struct PipedLines<R>(Option<R>);

impl<R> Lines for PipedLines<R>
where
    R: BufRead + Send + 'static,
{
    async fn next(&mut self) -> std::io::Result<Entry> {
        let Some(mut reader) = self.0.take() else {
            return Ok(Entry::Ended);
        };
        let (reader, entry) =
            tokio::task::spawn_blocking(move || -> std::io::Result<(R, Entry)> {
                let mut line = String::new();
                // A count of 0 is EOF and only EOF: a real line always carries at
                // least its terminator, so an empty `line` cannot mean anything
                // else.
                match reader.read_line(&mut line)? {
                    0 => Ok((reader, Entry::Ended)),
                    _ => Ok((reader, Entry::Statement(line))),
                }
            })
            .await
            .map_err(std::io::Error::other)??;
        self.0 = Some(reader);
        Ok(entry)
    }
}

/// The interactive half: reedline over the real terminal.
///
/// The editor moves onto a blocking thread and back for each read, for the
/// same reason [`PipedLines`] does.
struct EditorLines(Option<Reedline>);

impl EditorLines {
    fn new() -> Self {
        let editor = Reedline::create().with_validator(Box::new(SqlValidator));
        // History is a convenience, not a requirement: a session with nowhere
        // to write one still edits and recalls within itself.
        let editor = match history_path(|key| std::env::var(key).ok())
            .and_then(|path| FileBackedHistory::with_file(HISTORY_CAPACITY, path).ok())
        {
            Some(history) => editor.with_history(Box::new(history)),
            None => editor,
        };
        Self(Some(editor))
    }
}

impl Lines for EditorLines {
    async fn next(&mut self) -> std::io::Result<Entry> {
        let Some(mut editor) = self.0.take() else {
            return Ok(Entry::Ended);
        };
        let (editor, entry) = tokio::task::spawn_blocking(move || {
            let signal = editor.read_line(&ReplPrompt::new());
            (editor, signal)
        })
        .await
        .map_err(std::io::Error::other)?;
        self.0 = Some(editor);

        Ok(match entry? {
            Signal::Success(statement) => Entry::Statement(statement),
            Signal::CtrlD => Entry::Ended,
            // `Ctrl+C`, and anything a future reedline adds for an event
            // nothing here binds: abandon the line and prompt afresh. That
            // neither runs something the user did not ask for nor ends a
            // session the user did not end. `Signal` is `#[non_exhaustive]`,
            // so this arm is required rather than merely defensive.
            _ => Entry::Cancelled,
        })
    }
}

/// The editor's view of when a statement is finished.
struct SqlValidator;

impl Validator for SqlValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        match is_submittable(line) {
            true => ValidationResult::Complete,
            false => ValidationResult::Incomplete,
        }
    }
}

/// Whether the editor should hand `buffer` over rather than ask for another
/// line. Blank input and the exit words are not SQL, so completeness cannot
/// apply to them -- without this, `exit` would wait forever for a semicolon
/// it is never going to get.
fn is_submittable(buffer: &str) -> bool {
    let trimmed = buffer.trim();
    trimmed.is_empty() || is_exit_word(trimmed) || sql_is_complete(buffer)
}

/// Whether `sql` is a complete statement, according to SQLite's own
/// tokenizer: it ends at a `;` that is not inside a string literal, a
/// comment, or a `BEGIN ... END` body. Hand-rolling this is the classic way
/// to get `SELECT ';'` wrong.
fn sql_is_complete(sql: &str) -> bool {
    let Ok(text) = CString::new(sql) else {
        // An interior NUL cannot be handed to a C string. Call it complete so
        // the pipeline reports the problem, rather than the editor waiting
        // for a terminator the user has no way to supply.
        return true;
    };
    #[expect(
        unsafe_code,
        reason = "sqlite3_complete only tokenizes the NUL-terminated string it \
                  is given, which the CString above owns for the call"
    )]
    let complete = unsafe { rusqlite::ffi::sqlite3_complete(text.as_ptr()) };
    complete != 0
}

/// Where the editor keeps its history, resolved from `var` (the environment).
///
/// One file for every directory, mirroring `sqlite3`'s single
/// `~/.sqlite_history`: a query worked out in one project is worth recalling
/// in the next. `None` means the session keeps its history in memory.
fn history_path(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let non_empty = |key| var(key).filter(|value| !value.is_empty());

    if let Some(data_home) = non_empty("XDG_DATA_HOME") {
        return Some(PathBuf::from(data_home).join("dirsql").join("history"));
    }
    if let Some(home) = non_empty("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("dirsql")
                .join("history"),
        );
    }
    non_empty("APPDATA").map(|appdata| PathBuf::from(appdata).join("dirsql").join("history"))
}

/// How the interactive prompt is drawn: `dirsql> `, with `sqlite3`'s
/// continuation marker.
///
/// reedline's own prompt supplies the segments, the `> ` indicator and the
/// history-search line. Only the continuation marker is ours: a SQL shell's
/// users already know `...>` from `sqlite3`, and reedline's `::: ` would be
/// the one piece of the prompt they had to learn.
struct ReplPrompt(DefaultPrompt);

impl ReplPrompt {
    fn new() -> Self {
        Self(DefaultPrompt::new(
            DefaultPromptSegment::Basic(PROMPT_LEFT.to_string()),
            DefaultPromptSegment::Empty,
        ))
    }
}

impl Prompt for ReplPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        self.0.render_prompt_left()
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        self.0.render_prompt_right()
    }

    fn render_prompt_indicator(&self, edit_mode: PromptEditMode) -> Cow<'_, str> {
        self.0.render_prompt_indicator(edit_mode)
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        PROMPT_CONTINUATION.into()
    }

    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        self.0.render_prompt_history_search_indicator(search)
    }
}

/// `exit` and `quit` end the session. Neither is a valid SQL statement, so
/// there is nothing to namespace them against — which is why dirsql has no
/// dot-commands (#953).
fn is_exit_word(line: &str) -> bool {
    line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit")
}

/// The interactive greeting. Static text only: no scan and no live rows, so
/// the first prompt appears immediately even in a large tree (#986). The
/// samples carry their terminators, because typed statements end at one.
fn banner() -> String {
    format!(
        "dirsql {version} — this directory is a database.\n\
         \n  \
         SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5;\n  \
         SELECT path FROM './**/*.md' WHERE content LIKE '%TODO%';\n\
         \n\
         `exit`, `quit`, or Ctrl-D to leave.\n\n",
        version = env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
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

    /// A [`Lines`] double replaying a scripted sequence of entries, so the
    /// loop's handling of every one -- including `Ctrl+C`, which no plain
    /// reader can produce -- is exercised without a terminal.
    struct FakeLines(std::vec::IntoIter<std::io::Result<Entry>>);

    impl FakeLines {
        fn of(entries: Vec<Entry>) -> Self {
            Self(entries.into_iter().map(Ok).collect::<Vec<_>>().into_iter())
        }

        /// One statement per element, then EOF.
        fn statements(statements: &[&str]) -> Self {
            let mut entries: Vec<Entry> = statements
                .iter()
                .map(|s| Entry::Statement((*s).to_string()))
                .collect();
            entries.push(Entry::Ended);
            Self::of(entries)
        }

        fn failing() -> Self {
            Self(vec![Err(std::io::Error::other("input went away"))].into_iter())
        }
    }

    impl Lines for FakeLines {
        async fn next(&mut self) -> std::io::Result<Entry> {
            self.0.next().unwrap_or(Ok(Entry::Ended))
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

    /// A sink that accepts writes but cannot flush -- a greeting sitting in a
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

    /// An environment lookup over a fixed map, so history resolution is
    /// tested without reading the real one.
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    async fn drive(entries: Vec<Entry>) -> (FakeStatements, String, String, u8) {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = repl_loop(
            &fake,
            FakeLines::of(entries),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;
        (fake, text(&out), text(&err), code)
    }

    #[tokio::test]
    async fn every_non_blank_statement_is_executed_in_order() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = repl_loop(
            &fake,
            FakeLines::statements(&["SELECT 1", "SELECT 2"]),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(fake.seen(), vec!["SELECT 1", "SELECT 2"]);
        assert_eq!(text(&out), "[]\n[]\n", "each result on its own line");
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn statements_are_trimmed_before_execution() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(
            &fake,
            FakeLines::statements(&["   SELECT 1   \n"]),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(fake.seen(), vec!["SELECT 1"]);
    }

    #[tokio::test]
    async fn blank_and_whitespace_entries_are_skipped() {
        let (fake, out, err, code) = drive(vec![
            Entry::Statement("".into()),
            Entry::Statement("   ".into()),
            Entry::Statement("\t\n".into()),
            Entry::Ended,
        ])
        .await;

        assert!(fake.seen().is_empty(), "nothing was executed");
        assert_eq!(out, "", "a blank entry renders nothing");
        assert_eq!(err, "", "a blank entry is not an error");
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn exit_ends_the_session_before_the_rest_of_the_input() {
        let (fake, _, _, code) = drive(vec![
            Entry::Statement("SELECT 1".into()),
            Entry::Statement("exit".into()),
            Entry::Statement("SELECT 2".into()),
        ])
        .await;

        assert_eq!(fake.seen(), vec!["SELECT 1"]);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn quit_ends_the_session_before_the_rest_of_the_input() {
        let (fake, _, _, code) = drive(vec![
            Entry::Statement("quit".into()),
            Entry::Statement("SELECT 2".into()),
        ])
        .await;

        assert!(fake.seen().is_empty());
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn ctrl_c_abandons_the_line_and_keeps_the_session() {
        // #988's reason for routing the interrupt through the editor rather
        // than a signal handler: the process must survive it.
        let (fake, _, err, code) = drive(vec![
            Entry::Cancelled,
            Entry::Statement("SELECT 1".into()),
            Entry::Ended,
        ])
        .await;

        assert_eq!(fake.seen(), vec!["SELECT 1"], "the next line still runs");
        assert_eq!(err, "", "an abandoned line is not a failure");
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
            FakeLines::statements(&["SELECT nope", "SELECT 1"]),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(text(&err), "dirsql: no such table: nowhere\n");
        assert_eq!(text(&out), "[]\n", "errors never reach the result sink");
        assert_eq!(fake.seen(), vec!["SELECT nope", "SELECT 1"]);
        assert_eq!(code, 0, "a clean EOF exits 0 even after a failure");
    }

    #[tokio::test]
    async fn the_piped_path_writes_no_banner() {
        let (_, out, _, _) = drive(vec![Entry::Statement("SELECT 1".into()), Entry::Ended]).await;

        assert_eq!(out, "[]\n", "results alone, no interactive furniture");
    }

    #[tokio::test]
    async fn the_interactive_path_closes_with_a_newline() {
        // Ctrl-D leaves the cursor mid-line; without this the shell prompt
        // returns on top of the dirsql prompt.
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(
            &fake,
            FakeLines::of(vec![]),
            &mut out,
            &mut err,
            Format::Json,
            true,
        )
        .await;

        assert!(text(&out).ends_with("\n\n\n"), "{:?}", text(&out));
    }

    #[tokio::test]
    async fn the_interactive_path_opens_with_the_banner() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(
            &fake,
            FakeLines::of(vec![]),
            &mut out,
            &mut err,
            Format::Json,
            true,
        )
        .await;

        assert!(text(&out).starts_with(&banner()), "{:?}", text(&out));
    }

    #[tokio::test]
    async fn a_closed_sink_abandons_the_banner_rather_than_looping() {
        let fake = FakeStatements::default();
        let mut err = Vec::new();

        let code = repl_loop(
            &fake,
            FakeLines::statements(&["SELECT 1"]),
            &mut BrokenPipe,
            &mut err,
            Format::Json,
            true,
        )
        .await;

        assert_eq!(code, 1);
        assert!(fake.seen().is_empty());
    }

    #[tokio::test]
    async fn an_unflushable_banner_stops_the_session_too() {
        // A greeting that reaches the buffer but never the terminal leaves the
        // user staring at a blank screen, so it is as fatal as one that cannot
        // be written at all -- both halves of the write have to be checked.
        let fake = FakeStatements::default();
        let mut err = Vec::new();

        let code = repl_loop(
            &fake,
            FakeLines::statements(&["SELECT 1"]),
            &mut FlushFails,
            &mut err,
            Format::Json,
            true,
        )
        .await;

        assert_eq!(code, 1);
        assert!(fake.seen().is_empty(), "nothing runs behind a dead screen");
    }

    #[tokio::test]
    async fn a_read_failure_ends_the_session_with_an_error() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = repl_loop(
            &fake,
            FakeLines::failing(),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

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
            FakeLines::statements(&["SELECT 1", "SELECT 2"]),
            &mut BrokenPipe,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(code, 0);
        assert_eq!(fake.seen(), vec!["SELECT 1"], "the second never runs");
    }

    #[tokio::test]
    async fn the_piped_reader_yields_one_statement_per_line() {
        // The pipe half keeps slice 1's rule: a redirected script is not being
        // typed, so there is no terminator to wait for.
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(
            &fake,
            PipedLines(Some(input("SELECT 1\nSELECT 2\n"))),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(fake.seen(), vec!["SELECT 1", "SELECT 2"]);
    }

    #[tokio::test]
    async fn the_piped_reader_runs_a_final_line_without_a_newline() {
        // The last line of a heredoc or `printf` often arrives unterminated.
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        repl_loop(
            &fake,
            PipedLines(Some(input("SELECT 1"))),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(fake.seen(), vec!["SELECT 1"]);
    }

    #[tokio::test]
    async fn the_piped_reader_reports_a_read_failure() {
        let fake = FakeStatements::default();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = repl_loop(
            &fake,
            PipedLines(Some(BrokenReader)),
            &mut out,
            &mut err,
            Format::Json,
            false,
        )
        .await;

        assert_eq!(code, 1);
        assert_eq!(
            text(&err),
            "dirsql: failed to read input: input went away\n"
        );
    }

    #[tokio::test]
    async fn an_exhausted_piped_reader_stays_ended() {
        // The reader is moved out for each blocking read; if one is ever lost
        // the source must report EOF rather than being polled forever.
        let mut lines = PipedLines(None::<Cursor<Vec<u8>>>);

        assert!(matches!(lines.next().await, Ok(Entry::Ended)));
    }

    #[tokio::test]
    async fn a_degraded_index_is_reported_once_and_never_loops() {
        // `AppState::Unavailable` fails identically for every statement, so
        // repeating it per line is noise rather than information.
        let state = AppState::Unavailable("failed to resolve missing.toml".to_string());
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = run_repl(
            &state,
            Format::Json,
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
    fn a_terminated_statement_is_complete() {
        assert!(sql_is_complete("SELECT 1;"));
        assert!(sql_is_complete("SELECT 1;\n"));
    }

    #[test]
    fn an_unterminated_statement_is_incomplete() {
        // The multi-line case: the editor keeps asking until the semicolon.
        assert!(!sql_is_complete("SELECT 1"));
        assert!(!sql_is_complete("SELECT basename,"));
        assert!(!sql_is_complete("SELECT 1\nFROM t"));
    }

    #[test]
    fn a_semicolon_inside_a_string_literal_does_not_complete() {
        // The classic trap, and the reason this is SQLite's call rather than
        // a search for a trailing `;`.
        assert!(!sql_is_complete("SELECT ';'"));
        assert!(!sql_is_complete("SELECT 'a;b"));
        assert!(sql_is_complete("SELECT ';';"));
    }

    #[test]
    fn a_semicolon_inside_a_comment_does_not_complete() {
        assert!(!sql_is_complete("SELECT 1 -- ;"));
        assert!(!sql_is_complete("SELECT 1 /* ; */"));
        assert!(sql_is_complete("SELECT 1 /* ; */;"));
    }

    #[test]
    fn an_interior_nul_is_called_complete_so_the_pipeline_reports_it() {
        // A C string cannot carry it, and holding the line hostage for a
        // terminator the user cannot type would hang the session.
        assert!(sql_is_complete("SELECT\u{0}1;"));
    }

    #[test]
    fn a_complete_statement_is_submittable() {
        assert!(is_submittable("SELECT 1;"));
        assert!(!is_submittable("SELECT 1"));
    }

    #[test]
    fn exit_words_are_submittable_without_a_terminator() {
        // Neither is SQL, so waiting for a semicolon would hang the session on
        // the one input meant to end it.
        for word in ["exit", "quit", "  EXIT  "] {
            assert!(is_submittable(word), "{word} must submit as typed");
        }
    }

    #[test]
    fn blank_input_is_submittable() {
        // Pressing enter at a fresh prompt returns a fresh prompt; the loop
        // skips it. Holding it as incomplete would trap the user.
        assert!(is_submittable(""));
        assert!(is_submittable("   \n\t"));
    }

    #[test]
    fn the_validator_reports_the_submittable_decision() {
        assert!(matches!(
            SqlValidator.validate("SELECT 1;"),
            ValidationResult::Complete
        ));
        assert!(matches!(
            SqlValidator.validate("SELECT 1"),
            ValidationResult::Incomplete
        ));
    }

    #[test]
    fn history_prefers_the_xdg_data_home() {
        let path = history_path(env_of(&[
            ("XDG_DATA_HOME", "/data"),
            ("HOME", "/home/someone"),
        ]));

        assert_eq!(path, Some(PathBuf::from("/data/dirsql/history")));
    }

    #[test]
    fn history_falls_back_to_the_xdg_default_under_home() {
        let path = history_path(env_of(&[("HOME", "/home/someone")]));

        assert_eq!(
            path,
            Some(PathBuf::from("/home/someone/.local/share/dirsql/history"))
        );
    }

    #[test]
    fn history_falls_back_to_appdata() {
        let path = history_path(env_of(&[("APPDATA", "C:\\Users\\someone\\AppData")]));

        assert_eq!(
            path,
            Some(PathBuf::from("C:\\Users\\someone\\AppData/dirsql/history"))
        );
    }

    #[test]
    fn an_empty_variable_is_not_a_location() {
        // An exported-but-empty `XDG_DATA_HOME` would otherwise resolve to
        // `dirsql/history` relative to the invocation directory, writing a
        // history file into whatever tree the user happened to be querying.
        let path = history_path(env_of(&[("XDG_DATA_HOME", ""), ("HOME", "/home/someone")]));

        assert_eq!(
            path,
            Some(PathBuf::from("/home/someone/.local/share/dirsql/history"))
        );
    }

    #[test]
    fn no_location_means_an_in_memory_history() {
        assert_eq!(history_path(env_of(&[])), None);
    }

    #[test]
    fn the_prompt_renders_as_the_documented_string() {
        // `docs/reference/cli.md` shows `dirsql> `, which the editor draws as
        // the left segment followed by the indicator.
        let prompt = ReplPrompt::new();

        assert_eq!(
            format!(
                "{}{}",
                prompt.render_prompt_left(),
                prompt.render_prompt_indicator(PromptEditMode::Default)
            ),
            "dirsql> "
        );
    }

    #[test]
    fn the_prompt_forwards_the_segments_it_was_built_from() {
        // The wrapper exists to change the continuation marker and nothing
        // else. One that answered from its own constants would drift from
        // what the editor actually draws the moment either side changed.
        let prompt = ReplPrompt(DefaultPrompt::new(
            DefaultPromptSegment::Basic("left".to_string()),
            DefaultPromptSegment::Basic("right".to_string()),
        ));

        assert_eq!(prompt.render_prompt_left(), "left");
        assert_eq!(prompt.render_prompt_right(), "right");
    }

    #[test]
    fn nothing_is_drawn_at_the_right() {
        // Nothing here is worth the width: the directory is already the
        // user's shell prompt, and the session is bound to one.
        assert_eq!(ReplPrompt::new().render_prompt_right(), "");
    }

    #[test]
    fn the_continuation_prompt_differs_from_the_fresh_one() {
        // A user mid-statement must be able to see that enter will not run it.
        let prompt = ReplPrompt::new();

        assert_ne!(
            prompt.render_prompt_multiline_indicator(),
            prompt.render_prompt_indicator(PromptEditMode::Default)
        );
        assert_eq!(prompt.render_prompt_multiline_indicator(), "   ...> ");
    }

    #[test]
    fn the_history_search_indicator_names_the_term() {
        let prompt = ReplPrompt::new();

        let indicator = prompt.render_prompt_history_search_indicator(PromptHistorySearch {
            status: reedline::PromptHistorySearchStatus::Passing,
            term: "basename".to_string(),
        });

        assert_eq!(indicator, "(reverse-search: basename) ");
    }

    #[test]
    fn the_history_search_indicator_marks_a_miss() {
        // Without this the user cannot tell a stale match from no match.
        let prompt = ReplPrompt::new();

        let indicator = prompt.render_prompt_history_search_indicator(PromptHistorySearch {
            status: reedline::PromptHistorySearchStatus::Failing,
            term: "nope".to_string(),
        });

        assert_eq!(indicator, "(failing reverse-search: nope) ");
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
        // seen a path-table needs one here. They carry terminators, because
        // that is what the interactive path now requires.
        let banner = banner();

        assert!(
            banner.contains("SELECT basename, size FROM './'"),
            "{banner}"
        );
        assert!(banner.contains("FROM './**/*.md'"), "{banner}");
        for line in banner.lines().filter(|l| l.trim().starts_with("SELECT")) {
            assert!(line.trim_end().ends_with(';'), "{line:?} must terminate");
        }
    }

    #[test]
    fn the_banner_ends_with_a_blank_line_before_the_prompt() {
        assert!(banner().ends_with("\n\n"), "{}", banner());
    }
}
