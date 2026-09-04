//! Progress reporting for the startup scan.
//!
//! A cold scan over a large corpus can run for minutes -- the walk, then one
//! `on_file` round trip per file, then whatever the table's DDL triggers on
//! insert. None of that produced a byte of output before dirsql#957, so a user
//! had a hung-looking terminal and no way to tell a slow scan from a wedged
//! one.
//!
//! Two rules shape everything here:
//!
//! - **A pipe stays silent.** stdout carries query results and stderr carries
//!   diagnostics; a progress line is neither. Under the default the reporter
//!   writes nothing at all unless stderr is a terminal, so `| jq` pipelines,
//!   `2>` redirects and CI logs are byte-for-byte unchanged.
//! - **No new dependencies.** The line is a `\r`-rewritten counter, not a
//!   drawn bar: a bar wants the terminal width, and every portable way to ask
//!   for it is a crate. A counter reads the same at any width.
//!
//! [`Mode`] is the user's knob, read from `DIRSQL_PROGRESS`. [`Progress`] is
//! one phase's reporter: [`update`](Progress::update) while it runs and
//! [`finish`](Progress::finish) when it ends, which erases the live line and
//! leaves a single summary of what the phase cost -- the point of showing it.

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

/// The environment variable deciding whether progress is drawn.
pub const PROGRESS_ENV: &str = "DIRSQL_PROGRESS";

/// Redraw at most this often. The live line exists to prove the scan is
/// moving, which ten frames a second says as well as a thousand -- and a
/// thousand is a measurable cost of its own on a slow terminal.
const REDRAW_INTERVAL: Duration = Duration::from_millis(100);

/// Under [`Mode::Auto`], draw nothing until the phase has run this long. A
/// scan that finishes in a blink should leave the terminal exactly as it found
/// it; only work long enough to wonder about is worth reporting.
const WARMUP: Duration = Duration::from_millis(500);

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

/// The clock [`Progress`] throttles against. A seam so the unit tier can drive
/// the warmup and redraw thresholds from both sides instead of sleeping.
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
    out: Box<dyn Write + Send>,
    clock: Box<dyn Clock + Send>,
    mode: Mode,
    /// Whether the sink is a terminal. Only consulted under [`Mode::Auto`].
    terminal: bool,
    started: Instant,
    /// When the live line was last redrawn; `None` until the first draw, which
    /// is also what says whether a summary is owed.
    last_draw: Option<Instant>,
    /// Width of the line currently on screen, so the next one can overwrite
    /// its tail rather than leaving the trailing characters of a longer line
    /// behind.
    drawn_width: usize,
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
            out,
            clock,
            mode,
            terminal,
            started,
            last_draw: None,
            drawn_width: 0,
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

    /// Report `done` items complete, out of `total` when a total is known.
    /// Throttled, and under [`Mode::Auto`] silent until [`WARMUP`] has passed.
    pub fn update(&mut self, done: u64, total: Option<u64>) {
        if !self.enabled() {
            return;
        }
        let now = self.clock.now();
        match self.last_draw {
            Some(last) if now.duration_since(last) < REDRAW_INTERVAL => return,
            // The warmup gates only the FIRST draw: once a phase has proven
            // itself slow, it keeps reporting.
            None if self.mode == Mode::Auto && now.duration_since(self.started) < WARMUP => return,
            _ => {}
        }
        self.last_draw = Some(now);
        let line = render(self.label, self.noun, done, total, self.note.as_deref());
        self.draw(&line);
    }

    /// Reuse this reporter for a fresh phase: erase whatever is on screen and
    /// reset the clock and the throttle, keeping the sink, the mode and the
    /// wording. One reporter therefore serves every query on a connection --
    /// and, unlike constructing a new one per phase, it keeps whatever sink it
    /// was given instead of silently reverting to stderr.
    pub fn restart(&mut self) {
        self.erase();
        self.note = None;
        self.started = self.clock.now();
        self.last_draw = None;
    }

    /// End the phase: erase the live line and leave one summary line behind.
    /// Silent when nothing was ever drawn, so a fast phase leaves no trace.
    pub fn finish(&mut self, done: u64) {
        if self.last_draw.is_none() {
            return;
        }
        self.erase();
        let elapsed = self.clock.now().duration_since(self.started);
        let _ = writeln!(
            self.out,
            "dirsql: {} {done} {} in {}{}",
            self.summary_label,
            self.noun,
            format_duration(elapsed),
            parenthetical(self.note.as_deref())
        );
        let _ = self.out.flush();
    }

    fn draw(&mut self, line: &str) {
        let width = line.chars().count();
        let pad = self.drawn_width.saturating_sub(width);
        let _ = write!(self.out, "\r{line}{:pad$}", "");
        let _ = self.out.flush();
        self.drawn_width = width;
    }

    fn erase(&mut self) {
        if self.drawn_width == 0 {
            return;
        }
        let _ = write!(self.out, "\r{:width$}\r", "", width = self.drawn_width);
        self.drawn_width = 0;
    }
}

/// A phase that ends early -- a SQLite error mid-ingest, a hook that could not
/// be found -- must not leave a half-drawn counter under the error message the
/// caller is about to print. [`Progress::finish`] has already erased by the
/// time this runs, so on the normal path it does nothing.
impl Drop for Progress {
    fn drop(&mut self) {
        self.erase();
        let _ = self.out.flush();
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
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Sink {
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

    fn reporter(mode: Mode, terminal: bool) -> (Progress, Sink, FakeClock) {
        let sink = Sink::default();
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

        assert_eq!(sink.text(), "\rdirsql: indexing 3/10 files (30%)");
    }

    /// `always` is the explicit ask, so it skips the warmup entirely -- and it
    /// is what makes the drawn output observable from a test without a pty.
    #[test]
    fn always_draws_from_the_first_update() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.update(3, Some(10));

        assert_eq!(sink.text(), "\rdirsql: indexing 3/10 files (30%)");
    }

    /// The live line proves the scan is moving; redrawing it faster than the
    /// eye reads costs the terminal and says nothing new.
    #[test]
    fn updates_inside_the_redraw_interval_are_dropped() {
        let (mut progress, sink, clock) = reporter(Mode::Always, false);

        progress.update(1, Some(10));
        clock.advance(REDRAW_INTERVAL - Duration::from_millis(1));
        progress.update(2, Some(10));

        assert_eq!(
            sink.text(),
            "\rdirsql: indexing 1/10 files (10%)",
            "the second update is throttled away"
        );

        clock.advance(Duration::from_millis(1));
        progress.update(2, Some(10));

        assert_eq!(
            sink.text(),
            "\rdirsql: indexing 1/10 files (10%)\rdirsql: indexing 2/10 files (20%)"
        );
    }

    /// A shorter line must cover the tail of the longer one it replaces, or
    /// the terminal keeps showing digits from the previous count.
    #[test]
    fn a_shorter_line_is_padded_over_the_one_it_replaces() {
        let (mut progress, sink, clock) = reporter(Mode::Always, false);

        progress.update(1000, Some(1000));
        clock.advance(REDRAW_INTERVAL);
        progress.update(1, None);

        let long = "dirsql: indexing 1000/1000 files (100%)";
        let short = "dirsql: indexing 1 files";
        let pad = " ".repeat(long.len() - short.len());
        assert_eq!(sink.text(), format!("\r{long}\r{short}{pad}"));
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
            "\rdirsql: indexing 1/10 files (10%)\
             \r                                 \r\
             dirsql: indexed 10 files in 4.5s\n"
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

        let line = "dirsql: indexing 1/10 files (10%)";
        let blanks = " ".repeat(line.len());
        assert_eq!(
            sink.text(),
            format!("\r{line}\r{blanks}\r"),
            "restart erased the line, and the fresh phase is back under its warmup"
        );

        clock.advance(WARMUP);
        progress.update(2, Some(10));
        assert!(
            sink.text().ends_with("\rdirsql: indexing 2/10 files (20%)"),
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
        let line = "dirsql: indexing 1 files";
        assert_eq!(sink.text(), format!("\r{line}"), "the phase drew");

        CallProgress::restart(&mut progress);

        let blanks = " ".repeat(line.len());
        assert_eq!(
            sink.text(),
            format!("\r{line}\r{blanks}\r"),
            "restarting through the trait erased what it drew"
        );
    }

    /// An error mid-phase must not leave a half-drawn counter behind for the
    /// error message to land on top of.
    #[test]
    fn dropping_mid_phase_erases_the_live_line() {
        let (mut progress, sink, _clock) = reporter(Mode::Always, false);

        progress.update(1, Some(10));
        drop(progress);

        let line = "dirsql: indexing 1/10 files (10%)";
        let blanks = " ".repeat(line.len());
        assert_eq!(sink.text(), format!("\r{line}\r{blanks}\r"));
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
        let (mut progress, sink, clock) = reporter(Mode::Always, false);

        progress.set_note(Some("3 cached".to_string()));
        progress.update(1, None);
        progress.restart();
        clock.advance(REDRAW_INTERVAL);
        progress.update(1, None);

        assert!(
            sink.text().ends_with("dirsql: indexing 1 files"),
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

        assert_eq!(sink.text(), "\rdirsql: indexing 9204 files (8811 cached)");
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
