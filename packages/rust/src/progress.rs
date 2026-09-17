//! Progress reporting for the startup scan.
//!
//! A cold scan over a large corpus can run for minutes -- the walk, then one
//! `on_file` round trip per file, then whatever the table's DDL triggers on
//! insert. None of that produced a byte of output before dirsql#957, so a user
//! had a hung-looking terminal and no way to tell a slow scan from a wedged
//! one.
//!
//! **A pipe stays silent.** stdout carries query results and stderr carries
//! diagnostics; a progress line is neither. Under the default the reporter
//! writes nothing at all unless stderr is a terminal, so `| jq` pipelines,
//! `2>` redirects and CI logs are byte-for-byte unchanged. That gate is
//! [`Progress::enabled`] and it runs before any bar exists, so it holds
//! whatever the drawing library underneath would do on its own.
//!
//! The drawing and the redraw throttle are [`indicatif`]'s (dirsql#1081; the
//! "no new dependencies" rule this module used to carry is reversed -- a
//! redrawn counter is exactly the infrastructure a real crate covers). What
//! stays ours is the policy indicatif has no opinion about: the
//! `DIRSQL_PROGRESS` modes, the warmup before the first draw, the floored
//! percentage, and the summary line. indicatif refuses to draw to a
//! non-terminal and offers no override, so [`Mode::Always`] -- which exists to
//! make the output assertable without a pty -- is served by handing it a
//! [`TermLike`] over the reporter's own sink.
//!
//! [`Mode`] is the user's knob, read from `DIRSQL_PROGRESS`. [`Progress`] is
//! one phase's reporter: [`update`](Progress::update) while it runs and
//! [`finish`](Progress::finish) when it ends, which erases the live line and
//! leaves a single summary of what the phase cost -- the point of showing it.

use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle, TermLike};

/// The environment variable deciding whether progress is drawn.
pub const PROGRESS_ENV: &str = "DIRSQL_PROGRESS";

/// Redraw at most this often. The live line exists to prove the scan is
/// moving, which ten frames a second says as well as a thousand -- and a
/// thousand is a measurable cost of its own on a slow terminal.
const REDRAW_HZ: u8 = 10;

/// Under [`Mode::Auto`], draw nothing until the phase has run this long. A
/// scan that finishes in a blink should leave the terminal exactly as it found
/// it; only work long enough to wonder about is worth reporting.
const WARMUP: Duration = Duration::from_millis(500);

/// Line width assumed when the sink is not a terminal, which is every sink
/// [`Mode::Always`] draws to. A pipe has no width, and the drawn line has to
/// be padded to *some* number for a redraw to cover what it replaces.
const PIPE_WIDTH: u16 = 80;

/// Whether progress is drawn, and on what evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Draw only on a terminal, and only after [`WARMUP`]. The default.
    Auto,
    /// Draw regardless of terminal, from the first update. What a user sets to
    /// watch a scan whose stderr is redirected, and what makes the drawn
    /// output assertable from a test without a pty.
    Always,
    /// Draw nothing, ever. The opt-out for an embedder that owns its own
    /// terminal output.
    Never,
}

impl Mode {
    /// Parse the `DIRSQL_PROGRESS` value. Unset, `auto`, and anything
    /// unrecognized all mean [`Auto`](Mode::Auto): a typo in an environment
    /// variable should not decide whether a scan runs.
    pub fn parse(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "always" | "1" | "true" => Mode::Always,
            "never" | "0" | "false" => Mode::Never,
            _ => Mode::Auto,
        }
    }
}

/// The clock [`Progress`] times the warmup and the summary against. A seam so
/// the unit tier can drive the warmup threshold from both sides instead of
/// sleeping.
///
/// `Send` because a reporter is shared with the worker-call counter, which
/// SQLite invokes from whatever thread is running the query.
pub trait Clock: Send {
    fn now(&self) -> Instant;
}

/// The production clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// The reporter's sink, shared with the [`TermLike`] indicatif draws through.
/// `Mutex` rather than a plain `Box` because [`TermLike`] is `Sync` and takes
/// `&self`.
type Sink = Arc<Mutex<Box<dyn Write + Send>>>;

/// The terminal indicatif draws to: the reporter's own sink, at a fixed width.
///
/// Going through [`TermLike`] rather than `ProgressDrawTarget::stderr` is what
/// keeps [`Mode::Always`] working. indicatif checks `isatty` when it builds a
/// terminal target and silently hides the bar otherwise, with no override
/// ([indicatif#87], open since 2019), so a forced-on run with redirected
/// stderr would draw nothing.
///
/// The cursor never leaves the one live line, so the moves below are only ever
/// called with `n == 0` in this crate; they are spelled out anyway because the
/// trait is indicatif's to call. Clearing writes spaces rather than an erase
/// escape: `always` draws to redirected stderr, where an escape sequence is
/// noise in a file a human reads.
///
/// [indicatif#87]: https://github.com/console-rs/indicatif/issues/87
struct SinkTerm {
    sink: Sink,
    width: u16,
}

impl fmt::Debug for SinkTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SinkTerm")
            .field("width", &self.width)
            .finish()
    }
}

impl SinkTerm {
    fn put(&self, text: &str) -> io::Result<()> {
        self.sink.lock().unwrap().write_all(text.as_bytes())
    }

    fn seek(&self, n: usize, code: char) -> io::Result<()> {
        if n == 0 {
            return Ok(());
        }
        self.put(&format!("\x1b[{n}{code}"))
    }
}

impl TermLike for SinkTerm {
    fn width(&self) -> u16 {
        self.width
    }

    fn move_cursor_up(&self, n: usize) -> io::Result<()> {
        self.seek(n, 'A')
    }

    fn move_cursor_down(&self, n: usize) -> io::Result<()> {
        self.seek(n, 'B')
    }

    fn move_cursor_right(&self, n: usize) -> io::Result<()> {
        self.seek(n, 'C')
    }

    fn move_cursor_left(&self, n: usize) -> io::Result<()> {
        self.seek(n, 'D')
    }

    fn write_line(&self, line: &str) -> io::Result<()> {
        self.put(&format!("{line}\n"))
    }

    fn write_str(&self, text: &str) -> io::Result<()> {
        self.put(text)
    }

    fn clear_line(&self) -> io::Result<()> {
        self.put(&format!("\r{:width$}\r", "", width = self.width as usize))
    }

    fn flush(&self) -> io::Result<()> {
        self.sink.lock().unwrap().flush()
    }
}

/// The width a drawn line is padded to. A terminal knows its own; anything
/// else gets [`PIPE_WIDTH`].
fn line_width(terminal: bool) -> u16 {
    match terminal {
        true => console::Term::stderr().size().1,
        false => PIPE_WIDTH,
    }
}

/// One phase's progress reporter.
///
/// Construct with [`scanning`](Progress::scanning) or
/// [`indexing`](Progress::indexing), call [`update`](Progress::update) as the
/// phase advances, and [`finish`](Progress::finish) when it ends. Every method
/// is a no-op when the mode or the terminal says not to draw, so callers need
/// no gating of their own.
pub struct Progress {
    /// Present participle for the live line ("indexing 3/9 files").
    label: &'static str,
    /// Past participle for the summary ("indexed 9 files in 4.2s").
    summary_label: &'static str,
    /// What is being counted: "files", "worker calls".
    noun: &'static str,
    /// A parenthetical appended to the live line and the summary when the
    /// phase has something to add to its bare count. Deliberately a free
    /// string: the worker-call phase fills it with the cache split, and core
    /// stays ignorant of what any particular worker caches.
    note: Option<String>,
    sink: Sink,
    width: u16,
    clock: Box<dyn Clock + Send>,
    mode: Mode,
    /// Whether the sink is a terminal. Only consulted under [`Mode::Auto`].
    terminal: bool,
    started: Instant,
    /// The live bar, built on the first draw. `None` until then, which is also
    /// what says whether a summary is owed.
    bar: Option<ProgressBar>,
}

impl Progress {
    /// Reporter for the directory walk, which counts files as it finds them.
    pub fn scanning() -> Self {
        Self::to_stderr("scanning", "scanned", "files")
    }

    /// Reporter for the ingest pass, which counts files against a known total.
    pub fn indexing() -> Self {
        Self::to_stderr("indexing", "indexed", "files")
    }

    /// Reporter for a query's worker round trips. No total: the query decides
    /// how many rows it calls the function on, and SQLite does not say up
    /// front.
    pub fn worker_calls() -> Self {
        Self::to_stderr("running", "ran", "worker calls")
    }

    fn to_stderr(label: &'static str, summary_label: &'static str, noun: &'static str) -> Self {
        let terminal = std::io::stderr().is_terminal();
        Self::new(
            label,
            summary_label,
            noun,
            Box::new(std::io::stderr()),
            Box::new(SystemClock),
            Mode::parse(std::env::var(PROGRESS_ENV).ok().as_deref()),
            terminal,
        )
    }

    /// The constructor every seam goes through. `terminal` is the sink's
    /// terminal-ness, already resolved by the caller.
    pub fn new(
        label: &'static str,
        summary_label: &'static str,
        noun: &'static str,
        out: Box<dyn Write + Send>,
        clock: Box<dyn Clock + Send>,
        mode: Mode,
        terminal: bool,
    ) -> Self {
        let started = clock.now();
        Self {
            label,
            summary_label,
            noun,
            note: None,
            sink: Arc::new(Mutex::new(out)),
            width: line_width(terminal),
            clock,
            mode,
            terminal,
            started,
            bar: None,
        }
    }

    /// Set (or clear) the parenthetical the next draw carries.
    pub(crate) fn set_note(&mut self, note: Option<String>) {
        self.note = note;
    }

    /// Whether this reporter draws at all. [`Mode::Auto`] defers to the sink.
    fn enabled(&self) -> bool {
        match self.mode {
            Mode::Always => true,
            Mode::Never => false,
            Mode::Auto => self.terminal,
        }
    }

    /// The live bar: one line of our own text, redrawn at [`REDRAW_HZ`].
    fn bar(&self) -> ProgressBar {
        let term = SinkTerm {
            sink: Arc::clone(&self.sink),
            width: self.width,
        };
        let bar = ProgressBar::with_draw_target(
            None,
            ProgressDrawTarget::term_like_with_hz(Box::new(term), REDRAW_HZ),
        );
        bar.set_style(
            ProgressStyle::with_template("{msg}").expect("a literal template always parses"),
        );
        bar
    }

    /// Report `done` items complete, out of `total` when a total is known.
    /// Throttled, and under [`Mode::Auto`] silent until [`WARMUP`] has passed.
    pub fn update(&mut self, done: u64, total: Option<u64>) {
        if !self.enabled() {
            return;
        }
        if self.bar.is_none() {
            // The warmup gates only the FIRST draw: once a phase has proven
            // itself slow, it keeps reporting.
            if self.mode == Mode::Auto && self.clock.now().duration_since(self.started) < WARMUP {
                return;
            }
            self.bar = Some(self.bar());
        }
        let line = render(self.label, self.noun, done, total, self.note.as_deref());
        if let Some(bar) = &self.bar {
            bar.set_message(line);
        }
    }

    /// Reuse this reporter for a fresh phase: erase whatever is on screen and
    /// reset the clock, keeping the sink, the mode and the wording. One
    /// reporter therefore serves every query on a connection -- and, unlike
    /// constructing a new one per phase, it keeps whatever sink it was given
    /// instead of silently reverting to stderr.
    pub fn restart(&mut self) {
        self.erase();
        self.note = None;
        self.started = self.clock.now();
    }

    /// End the phase: erase the live line and leave one summary line behind.
    /// Silent when nothing was ever drawn, so a fast phase leaves no trace.
    ///
    /// The summary goes to the sink directly rather than through the bar:
    /// indicatif's own `println` leaves the line unterminated once the bar is
    /// cleared, and what survives the phase has to be a whole line.
    pub fn finish(&mut self, done: u64) {
        if !self.erase() {
            return;
        }
        let elapsed = self.clock.now().duration_since(self.started);
        let mut sink = self.sink.lock().unwrap();
        let _ = writeln!(
            sink,
            "dirsql: {} {done} {} in {}{}",
            self.summary_label,
            self.noun,
            format_duration(elapsed),
            parenthetical(self.note.as_deref())
        );
        let _ = sink.flush();
    }

    /// Clear the live line, reporting whether there was one.
    fn erase(&mut self) -> bool {
        match self.bar.take() {
            Some(bar) => {
                bar.finish_and_clear();
                true
            }
            None => false,
        }
    }
}

/// A phase that ends early -- a SQLite error mid-ingest, a hook that could not
/// be found -- must not leave a half-drawn counter under the error message the
/// caller is about to print. [`Progress::finish`] has already erased by the
/// time this runs, so on the normal path it does nothing.
impl Drop for Progress {
    fn drop(&mut self) {
        self.erase();
        let _ = self.sink.lock().unwrap().flush();
    }
}

/// The narrow view of a reporter that the worker-call counter needs: a running
/// count with no total, and a phase it can restart. A trait so the counter's
/// unit tests can inject a double without reaching across modules.
pub(crate) trait CallProgress: Send {
    /// Report `done` round trips so far, `cached` of which the worker said it
    /// served from its own cache. There is no total — SQLite does not say up
    /// front how many rows the query will call the function on.
    fn update(&mut self, done: u64, cached: u64);
    fn finish(&mut self, done: u64, cached: u64);
    fn restart(&mut self);
}

impl CallProgress for Progress {
    fn update(&mut self, done: u64, cached: u64) {
        self.set_note(cached_note(cached));
        Progress::update(self, done, None);
    }

    fn finish(&mut self, done: u64, cached: u64) {
        self.set_note(cached_note(cached));
        Progress::finish(self, done);
    }

    fn restart(&mut self) {
        Progress::restart(self);
    }
}

/// The cache split, shown only once there is one. A run that hit no cache
/// reads exactly as it did before the split existed, rather than carrying a
/// `(0 cached)` that answers a question nobody asked.
fn cached_note(cached: u64) -> Option<String> {
    (cached > 0).then(|| format!("{cached} cached"))
}

/// The live line's text. With a total it carries a percentage; the walk has no
/// total to divide by until it is over, so it reports a running count.
fn render(label: &str, noun: &str, done: u64, total: Option<u64>, note: Option<&str>) -> String {
    let counted = match total {
        Some(total) => format!("{done}/{total} {noun} ({}%)", percent(done, total)),
        None => format!("{done} {noun}"),
    };
    format!("dirsql: {label} {counted}{}", parenthetical(note))
}

/// A note as it appears on a line — ` (8811 cached)` — or nothing at all.
fn parenthetical(note: Option<&str>) -> String {
    note.map(|note| format!(" ({note})")).unwrap_or_default()
}

/// `done` as a percentage of `total`, floored. An empty total is complete by
/// definition rather than a division by zero.
fn percent(done: u64, total: u64) -> u64 {
    if total == 0 {
        return 100;
    }
    done.saturating_mul(100) / total
}

/// Elapsed time at the precision a human reads: tenths under a minute, whole
/// seconds above it.
fn format_duration(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{:.1}s", elapsed.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A `Write` the test can read back. Shares one buffer with the `Progress`
    /// that owns its clone. `Arc`/`Mutex` rather than `Rc`/`RefCell` because
    /// the sink has to satisfy the reporter's `Send` bound.
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A clock the test advances by hand, so the warmup and redraw thresholds
    /// can be driven from both sides without sleeping.
    #[derive(Clone)]
    struct FakeClock {
        base: Instant,
        offset: Arc<Mutex<Duration>>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                offset: Arc::new(Mutex::new(Duration::ZERO)),
            }
        }

        fn advance(&self, delta: Duration) {
            let mut offset = self.offset.lock().unwrap();
            *offset += delta;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.base + *self.offset.lock().unwrap()
        }
    }

    fn reporter(mode: Mode, terminal: bool) -> (Progress, Buffer, FakeClock) {
        let sink = Buffer::default();
        let clock = FakeClock::new();
        let progress = Progress::new(
            "indexing",
            "indexed",
            "files",
            Box::new(sink.clone()),
            Box::new(clock.clone()),
            mode,
            terminal,
        );
        (progress, sink, clock)
    }

    /// The sink's width, which is what a redraw clears and pads to. Every
    /// byte-exact test below drives a non-terminal sink, so the number is
    /// fixed rather than the host terminal's.
    const WIDTH: usize = PIPE_WIDTH as usize;

    /// A cleared line: the width blanked, cursor back at the start.
    fn blank() -> String {
        format!("\r{:WIDTH$}\r", "")
    }

    /// One drawn line as it reaches the sink, padded out to the full width so
    /// it covers whatever shared the row. A *re*draw is preceded by
    /// [`blank`]; the first draw of a phase has nothing to clear.
    fn drawn(line: &str) -> String {
        format!("{line}{:pad$}", "", pad = WIDTH - line.chars().count())
    }

    #[test]
    fn an_unset_variable_means_auto() {
        assert_eq!(Mode::parse(None), Mode::Auto);
    }

    #[test]
    fn auto_is_spellable() {
        assert_eq!(Mode::parse(Some("auto")), Mode::Auto);
    }

    #[test]
    fn always_has_three_spellings() {
        assert_eq!(Mode::parse(Some("always")), Mode::Always);
        assert_eq!(Mode::parse(Some("1")), Mode::Always);
        assert_eq!(Mode::parse(Some("true")), Mode::Always);
    }

    #[test]
    fn never_has_three_spellings() {
        assert_eq!(Mode::parse(Some("never")), Mode::Never);
        assert_eq!(Mode::parse(Some("0")), Mode::Never);
        assert_eq!(Mode::parse(Some("false")), Mode::Never);
    }

    #[test]
    fn parsing_ignores_case_and_surrounding_space() {
        assert_eq!(Mode::parse(Some("  ALWAYS ")), Mode::Always);
        assert_eq!(Mode::parse(Some("\tNever\n")), Mode::Never);
    }

    /// A typo in an environment variable decides nothing: the scan runs, with
    /// the default policy.
    #[test]
    fn an_unrecognized_value_falls_back_to_auto() {
        assert_eq!(Mode::parse(Some("banana")), Mode::Auto);
        assert_eq!(Mode::parse(Some("")), Mode::Auto);
    }

    #[test]
    fn the_system_clock_moves_forward() {
        let clock = SystemClock;
        let first = clock.now();
        assert!(clock.now() >= first);
    }

    /// The headline default: a piped run writes nothing, which is what keeps
    /// progress out of `| jq` pipelines and CI logs.
    #[test]
    fn auto_draws_nothing_when_the_sink_is_not_a_terminal() {
        let (mut progress, sink, clock) = reporter(Mode::Auto, false);

        clock.advance(Duration::from_secs(30));
        progress.update(5, Some(10));
        progress.finish(10);

        assert_eq!(sink.text(), "");
    }

    #[test]
    fn never_draws_nothing_even_on_a_terminal() {
        let (mut progress, sink, clock) = reporter(Mode::Never, true);

        clock.advance(Duration::from_secs(30));
        progress.update(5, Some(10));
        progress.finish(10);

        assert_eq!(sink.text(), "");
    }

    /// Under `auto` a phase must prove itself slow before it draws: a scan
    /// that finishes in a blink leaves the terminal exactly as it found it.
    #[test]
    fn auto_stays_silent_until_the_warmup_has_elapsed() {
        let (mut progress, sink, clock) = reporter(Mode::Auto, true);

        progress.update(1, Some(10));
        clock.advance(WARMUP - Duration::from_millis(1));
        progress.update(2, Some(10));

        assert_eq!(sink.text(), "", "nothing is drawn before the warmup");

        clock.advance(Duration::from_millis(1));
        progress.update(3, Some(10));

        assert!(
            sink.text()
                .trim_end()
                .ends_with("dirsql: indexing 3/10 files (30%)"),
            "got: {:?}",
            sink.text()
        );
    }

    /// `always` is the explicit ask, so it skips the warmup entirely -- and it
    /// is what makes the drawn output observable from a test without a pty.
    #[test]
    fn always_draws_from_the_first_update() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.update(3, Some(10));

        assert_eq!(sink.text(), drawn("dirsql: indexing 3/10 files (30%)"));
    }

    /// A shorter line must cover the tail of the longer one it replaces, or
    /// the terminal keeps showing digits from the previous count. Clearing to
    /// the full line width rather than to the previous draw's width also
    /// covers whatever else shares the line -- a shell prompt, an editor's
    /// status line -- instead of only what this reporter itself wrote.
    #[test]
    fn a_shorter_line_is_padded_to_the_full_line_width() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.update(1000, Some(1000));
        progress.update(1, None);

        let short = "dirsql: indexing 1 files";
        let padded = format!("{short}{}", " ".repeat(80 - short.len()));
        assert!(
            sink.text().ends_with(&padded),
            "the redraw clears the whole line, not just the previous draw's tail: {:?}",
            sink.text()
        );
    }

    /// A phase that never drew leaves no trace -- no erase, no summary.
    #[test]
    fn finishing_without_a_draw_writes_nothing() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.finish(10);

        assert_eq!(sink.text(), "");
    }

    /// What survives the phase is one line saying what it cost. Erasing first
    /// keeps the live counter from being left behind mid-count.
    #[test]
    fn finishing_erases_the_live_line_and_summarizes_the_cost() {
        let (mut progress, sink, clock) = reporter(Mode::Always, false);

        progress.update(1, Some(10));
        clock.advance(Duration::from_millis(4500));
        progress.finish(10);

        assert_eq!(
            sink.text(),
            format!(
                "{}{}dirsql: indexed 10 files in 4.5s\n",
                drawn("dirsql: indexing 1/10 files (10%)"),
                blank()
            )
        );
    }

    #[test]
    fn a_phase_with_no_known_total_reports_a_running_count() {
        assert_eq!(
            render("scanning", "files", 42, None, None),
            "dirsql: scanning 42 files"
        );
    }

    #[test]
    fn a_phase_with_a_known_total_reports_a_percentage() {
        assert_eq!(
            render("indexing", "files", 3, Some(8), None),
            "dirsql: indexing 3/8 files (37%)"
        );
    }

    /// The noun travels with the phase: worker round trips are not files, and
    /// a line that called them files would be lying about what it counted.
    #[test]
    fn the_counted_thing_is_named_by_the_phase() {
        assert_eq!(
            render("running", "worker calls", 9204, None, None),
            "dirsql: running 9204 worker calls"
        );
    }

    #[test]
    fn a_percentage_floors_rather_than_rounds() {
        assert_eq!(percent(3, 8), 37);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(9, 10), 90);
    }

    /// An empty total is complete by definition rather than a division by
    /// zero.
    #[test]
    fn an_empty_total_is_a_hundred_percent() {
        assert_eq!(percent(0, 0), 100);
    }

    /// The multiplication is saturating, so a count near the integer ceiling
    /// reports a bounded number instead of wrapping to a small one.
    #[test]
    fn an_enormous_count_does_not_wrap() {
        assert_eq!(percent(u64::MAX, u64::MAX), 1);
    }

    #[test]
    fn a_short_duration_reads_in_tenths_of_a_second() {
        assert_eq!(format_duration(Duration::from_millis(40)), "0.0s");
        assert_eq!(format_duration(Duration::from_millis(4500)), "4.5s");
        assert_eq!(format_duration(Duration::from_millis(59_900)), "59.9s");
    }

    #[test]
    fn a_long_duration_reads_in_minutes_and_seconds() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1m00s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2m05s");
        assert_eq!(format_duration(Duration::from_secs(3725)), "62m05s");
    }

    /// A reporter serves more than one phase, so restarting must forget the
    /// previous phase's clock and throttle -- otherwise the second phase
    /// inherits the first one's elapsed time and draws immediately.
    #[test]
    fn restarting_clears_the_line_and_the_clock() {
        let (mut progress, sink, clock) = reporter(Mode::Auto, true);

        clock.advance(WARMUP);
        progress.update(1, Some(10));
        let drawn = sink.text();
        assert!(!drawn.is_empty(), "the first phase drew");

        progress.restart();
        progress.update(1, Some(10));

        assert!(
            sink.text().ends_with('\r')
                && sink.text().trim_end_matches([' ', '\r']).ends_with("(10%)"),
            "restart erased the line, and the fresh phase is back under its warmup: {:?}",
            sink.text()
        );

        clock.advance(WARMUP);
        progress.update(2, Some(10));
        assert!(
            sink.text()
                .trim_end()
                .ends_with("dirsql: indexing 2/10 files (20%)"),
            "and it draws again once the new phase is old enough: {:?}",
            sink.text()
        );
    }

    /// The `CallProgress` impl is a delegation, and a delegation that quietly
    /// does nothing looks identical to a working one -- until a second query
    /// inherits the first one's line.
    #[test]
    fn restarting_through_call_progress_erases_the_live_line() {
        let (mut progress, sink, clock) = reporter(Mode::Auto, true);

        clock.advance(WARMUP);
        CallProgress::update(&mut progress, 1, 0);
        assert!(
            sink.text().trim_end().ends_with("dirsql: indexing 1 files"),
            "the phase drew: {:?}",
            sink.text()
        );

        CallProgress::restart(&mut progress);

        assert!(
            sink.text().ends_with('\r'),
            "restarting through the trait erased what it drew: {:?}",
            sink.text()
        );
    }

    /// An error mid-phase must not leave a half-drawn counter behind for the
    /// error message to land on top of.
    #[test]
    fn dropping_mid_phase_erases_the_live_line() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.update(1, Some(10));
        drop(progress);

        assert_eq!(
            sink.text(),
            format!("{}{}", drawn("dirsql: indexing 1/10 files (10%)"), blank())
        );
    }

    /// ...and a phase that already finished has nothing left to erase, so the
    /// summary is the last thing written.
    #[test]
    fn dropping_after_finishing_writes_nothing_further() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.update(1, Some(10));
        progress.finish(10);
        let after_finish = sink.text();
        drop(progress);

        assert_eq!(sink.text(), after_finish);
    }

    /// The cache split rides on the same line as the count it qualifies, so a
    /// user reads "how much work" and "how much of it was free" at once.
    #[test]
    fn a_note_is_appended_to_the_live_line_in_parentheses() {
        assert_eq!(
            render("running", "worker calls", 9204, None, Some("8811 cached")),
            "dirsql: running 9204 worker calls (8811 cached)"
        );
    }

    /// A phase with a total keeps its percentage and gains the note after it.
    #[test]
    fn a_note_follows_the_percentage_when_there_is_a_total() {
        assert_eq!(
            render("indexing", "files", 3, Some(8), Some("2 skipped")),
            "dirsql: indexing 3/8 files (37%) (2 skipped)"
        );
    }

    #[test]
    fn no_note_means_no_parentheses() {
        assert_eq!(parenthetical(None), "");
        assert_eq!(parenthetical(Some("8811 cached")), " (8811 cached)");
    }

    /// The summary is the line that survives the phase, so the split has to
    /// reach it -- and it goes after the elapsed time, which is what the
    /// sentence is about.
    #[test]
    fn the_summary_carries_the_note_after_the_elapsed_time() {
        let (mut progress, sink, clock) = reporter(Mode::Always, false);

        progress.update(1, Some(10));
        progress.set_note(Some("3 cached".to_string()));
        clock.advance(Duration::from_millis(4500));
        progress.finish(10);

        assert!(
            sink.text()
                .ends_with("dirsql: indexed 10 files in 4.5s (3 cached)\n"),
            "got: {:?}",
            sink.text()
        );
    }

    /// A note belongs to the phase that set it. A second query must not inherit
    /// the first one's cache split.
    #[test]
    fn restarting_clears_the_note() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.set_note(Some("3 cached".to_string()));
        progress.update(1, None);
        progress.restart();
        progress.update(1, None);

        assert!(
            sink.text().trim_end().ends_with("dirsql: indexing 1 files"),
            "the fresh phase draws no note: {:?}",
            sink.text()
        );
    }

    /// The worker-call adapter is what turns a cache count into words, and
    /// zero hits must read exactly as it did before the split existed.
    #[test]
    fn no_cache_hits_produce_no_note() {
        assert_eq!(cached_note(0), None);
        assert_eq!(cached_note(8811), Some("8811 cached".to_string()));
    }

    #[test]
    fn call_progress_updates_carry_the_cache_split() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        CallProgress::update(&mut progress, 9204, 8811);

        assert_eq!(
            sink.text(),
            drawn("dirsql: indexing 9204 files (8811 cached)")
        );
    }

    /// ...and so does the summary, which is where dirsql#1034's headline line
    /// actually lands.
    #[test]
    fn call_progress_finishes_with_the_cache_split() {
        let (mut progress, sink, clock) = reporter(Mode::Always, false);

        CallProgress::update(&mut progress, 4, 0);
        clock.advance(Duration::from_millis(2000));
        CallProgress::finish(&mut progress, 9204, 8811);

        assert!(
            sink.text()
                .ends_with("dirsql: indexed 9204 files in 2.0s (8811 cached)\n"),
            "got: {:?}",
            sink.text()
        );
    }

    /// The two production reporters differ only in wording, and the wording is
    /// what a user reads to tell the walk from the ingest.
    fn sink_term(sink: &Buffer) -> SinkTerm {
        SinkTerm {
            sink: Arc::new(Mutex::new(Box::new(sink.clone()) as Box<dyn Write + Send>)),
            width: 12,
        }
    }

    /// The reporter never leaves its one live line, so indicatif asks for a
    /// move of zero -- which has to cost no bytes, or every frame of a piped
    /// `always` run carries an escape sequence nobody can read.
    #[test]
    fn a_cursor_move_of_zero_writes_nothing() {
        let sink = Buffer::default();
        let term = sink_term(&sink);

        term.move_cursor_up(0).unwrap();
        term.move_cursor_down(0).unwrap();
        term.move_cursor_right(0).unwrap();
        term.move_cursor_left(0).unwrap();

        assert_eq!(sink.text(), "");
    }

    /// A real move is still a real move: indicatif owns when it asks.
    #[test]
    fn a_cursor_move_writes_the_escape_for_its_direction() {
        let sink = Buffer::default();
        let term = sink_term(&sink);

        term.move_cursor_up(1).unwrap();
        term.move_cursor_down(2).unwrap();
        term.move_cursor_right(3).unwrap();
        term.move_cursor_left(4).unwrap();

        assert_eq!(sink.text(), "\x1b[1A\x1b[2B\x1b[3C\x1b[4D");
    }

    #[test]
    fn writing_a_line_terminates_it() {
        let sink = Buffer::default();
        let term = sink_term(&sink);

        term.write_str("bare").unwrap();
        term.write_line("whole").unwrap();
        term.flush().unwrap();

        assert_eq!(sink.text(), "barewhole\n");
    }

    /// Clearing blanks the whole width with spaces rather than an erase
    /// escape, because `always` draws to redirected stderr.
    #[test]
    fn clearing_blanks_the_width_without_an_escape() {
        let sink = Buffer::default();
        let term = sink_term(&sink);

        term.clear_line().unwrap();

        assert_eq!(sink.text(), "\r            \r");
        assert_eq!(term.width(), 12);
    }

    #[test]
    fn the_terminal_debugs_as_its_width() {
        let sink = Buffer::default();

        assert_eq!(format!("{:?}", sink_term(&sink)), "SinkTerm { width: 12 }");
    }

    /// A pipe has no width, so it gets the fixed one; a terminal reports its
    /// own, which is only ever positive.
    #[test]
    fn a_pipe_gets_the_fixed_width() {
        assert_eq!(line_width(false), PIPE_WIDTH);
        assert!(line_width(true) > 0);
    }

    #[test]
    fn the_production_reporters_carry_the_phase_wording() {
        let scanning = Progress::scanning();
        assert_eq!(scanning.label, "scanning");
        assert_eq!(scanning.summary_label, "scanned");
        assert_eq!(scanning.noun, "files");

        let indexing = Progress::indexing();
        assert_eq!(indexing.label, "indexing");
        assert_eq!(indexing.summary_label, "indexed");
        assert_eq!(indexing.noun, "files");

        let calls = Progress::worker_calls();
        assert_eq!(calls.label, "running");
        assert_eq!(calls.summary_label, "ran");
        assert_eq!(calls.noun, "worker calls");
    }
}
