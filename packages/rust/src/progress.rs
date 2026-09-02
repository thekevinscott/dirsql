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
pub trait Clock {
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
    out: Box<dyn Write>,
    clock: Box<dyn Clock>,
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
        Self::to_stderr("scanning", "scanned")
    }

    /// Reporter for the ingest pass, which counts files against a known total.
    pub fn indexing() -> Self {
        Self::to_stderr("indexing", "indexed")
    }

    fn to_stderr(label: &'static str, summary_label: &'static str) -> Self {
        let terminal = std::io::stderr().is_terminal();
        Self::new(
            label,
            summary_label,
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
        out: Box<dyn Write>,
        clock: Box<dyn Clock>,
        mode: Mode,
        terminal: bool,
    ) -> Self {
        let started = clock.now();
        Self {
            label,
            summary_label,
            out,
            clock,
            mode,
            terminal,
            started,
            last_draw: None,
            drawn_width: 0,
        }
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
        let line = render(self.label, done, total);
        self.draw(&line);
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
            "dirsql: {} {done} files in {}",
            self.summary_label,
            format_duration(elapsed)
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

/// The live line's text. With a total it carries a percentage; the walk has no
/// total to divide by until it is over, so it reports a running count.
fn render(label: &str, done: u64, total: Option<u64>) -> String {
    match total {
        Some(total) => format!(
            "dirsql: {label} {done}/{total} files ({}%)",
            percent(done, total)
        ),
        None => format!("dirsql: {label} {done} files"),
    }
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
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// A `Write` the test can read back. Shares one buffer with the `Progress`
    /// that owns its clone.
    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Vec<u8>>>);

    impl Sink {
        fn text(&self) -> String {
            String::from_utf8(self.0.borrow().clone()).unwrap()
        }
    }

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
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
        offset: Rc<Cell<Duration>>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                offset: Rc::new(Cell::new(Duration::ZERO)),
            }
        }

        fn advance(&self, delta: Duration) {
            self.offset.set(self.offset.get() + delta);
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.base + self.offset.get()
        }
    }

    fn reporter(mode: Mode, terminal: bool) -> (Progress, Sink, FakeClock) {
        let sink = Sink::default();
        let clock = FakeClock::new();
        let progress = Progress::new(
            "indexing",
            "indexed",
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
        assert_eq!(render("scanning", 42, None), "dirsql: scanning 42 files");
    }

    #[test]
    fn a_phase_with_a_known_total_reports_a_percentage() {
        assert_eq!(
            render("indexing", 3, Some(8)),
            "dirsql: indexing 3/8 files (37%)"
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

    /// The two production reporters differ only in wording, and the wording is
    /// what a user reads to tell the walk from the ingest.
    #[test]
    fn the_production_reporters_carry_the_phase_wording() {
        let scanning = Progress::scanning();
        assert_eq!(scanning.label, "scanning");
        assert_eq!(scanning.summary_label, "scanned");

        let indexing = Progress::indexing();
        assert_eq!(indexing.label, "indexing");
        assert_eq!(indexing.summary_label, "indexed");
    }
}
