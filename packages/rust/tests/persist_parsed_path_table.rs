//! Integration tests for the persistent cache behind a *parsed* path-table —
//! the `--on-file` form of `SELECT ... FROM './**/*.json'`.
//!
//! `docs/howto/persist.md` promises that `--persist` turns a restart into
//! "the difference between re-running and skipping expensive `on-file`
//! commands". These tests hold that promise to a path-table, the surface a
//! configless `dirsql --persist` actually queries: a second run over an
//! unchanged tree must re-run the parser for no file, must leave the cache
//! byte-for-byte alone, and must cost a fraction of the first.
//!
//! Real core, real SQLite, real temp-dir filesystem, real parser processes.

use dirsql::DirSQL;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tempfile::TempDir;

/// Files in the synthetic corpus for the timing test. The parser spawns one
/// process per file, so this is sized to make per-file parsing, not the fixed
/// cost of a run, the bulk of the cold run's work.
const CORPUS: usize = 4_000;

/// The second run must cost no more than this share of the first.
const WARM_RATIO: f64 = 0.10;

const SQL: &str = "SELECT id, tag FROM './**/*.json'";

/// The default parser body: hand the file straight back as the row payload.
const ECHO_FILE: &str = "cat \"$1\"";

/// A temp tree, a cache path outside it, and a parser that counts its own runs.
struct Fixture {
    root: TempDir,
    side: TempDir,
    cache: PathBuf,
    counter: PathBuf,
}

impl Fixture {
    /// Write `count` deterministic JSON files, each a single-row payload the
    /// parser can hand straight back. Seeded by index, never random, so two
    /// runs are comparable.
    fn with_corpus(count: usize) -> Self {
        let root = TempDir::new().unwrap();
        let side = TempDir::new().unwrap();
        let fixture = Self {
            cache: side.path().join("cache.db"),
            counter: side.path().join("parses"),
            root,
            side,
        };
        for i in 0..count {
            let path = fixture.file(i);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, payload(i, "v1")).unwrap();
        }
        fixture
    }

    fn file(&self, i: usize) -> PathBuf {
        self.root
            .path()
            .join(format!("d{:02}", i % 50))
            .join(format!("f{i}.json"))
    }

    /// Install a parser that records one byte per invocation before emitting
    /// `body`. Counting the parser's own runs is the read instrumentation: a
    /// file served from the cache never reaches it.
    fn parser(&self, name: &str, body: &str) -> String {
        let script = self.side.path().join(name);
        fs::write(&script, format!("printf x >> \"$2\"\n{body}\n")).unwrap();
        format!(
            "sh {} {{path}} {}",
            script.display(),
            self.counter.display()
        )
    }

    /// Parser invocations since the last [`Self::reset_counter`].
    fn parses(&self) -> usize {
        fs::metadata(&self.counter)
            .map(|m| usize::try_from(m.len()).unwrap())
            .unwrap_or(0)
    }

    fn reset_counter(&self) {
        fs::write(&self.counter, b"").unwrap();
    }

    /// Run one query against a parsed path-table, timing the whole
    /// build-and-query the way a `dirsql` invocation does.
    fn run(&self, parser: &str, persist: bool) -> (Vec<String>, Duration) {
        let started = Instant::now();
        let mut builder = DirSQL::builder()
            .root(self.root.path())
            .path_table_parser(parser);
        if persist {
            builder = builder.persist(Some(&self.cache));
        }
        let db = builder.build().unwrap();
        let rows = db.query(SQL).unwrap();
        let elapsed = started.elapsed();

        let mut out: Vec<String> = rows
            .iter()
            .map(|r| format!("{:?}|{:?}", r["id"], r["tag"]))
            .collect();
        out.sort();
        (out, elapsed)
    }

    fn cache_size(&self) -> u64 {
        fs::metadata(&self.cache).unwrap().len()
    }

    fn cache_digest(&self) -> [u8; 32] {
        *blake3::hash(&fs::read(&self.cache).unwrap()).as_bytes()
    }
}

fn payload(i: usize, tag: &str) -> String {
    format!(
        "[{{\"id\":{i},\"tag\":\"{tag}\",\"body\":\"{}\"}}]",
        "x".repeat(120 + i % 17)
    )
}

/// Push a file's mtime clear of the racy window so the stat tuple alone
/// decides whether it changed.
fn touch_into_the_future(path: &Path) {
    let future = SystemTime::now() + Duration::from_secs(5);
    fs::File::open(path).unwrap().set_modified(future).unwrap();
}

/// Pin a file's mtime far enough ahead that every cache written during the
/// test predates it. That is the racy window: the cache write does not postdate
/// the file's mtime, so the stat tuple alone cannot settle whether the file
/// changed and the reuse decision falls through to the content hash.
fn pin_inside_the_racy_window(path: &Path) -> SystemTime {
    let pinned = SystemTime::now() + Duration::from_secs(3600);
    fs::File::open(path).unwrap().set_modified(pinned).unwrap();
    pinned
}

#[test]
fn unchanged_second_run_reuses_the_cache_instead_of_reparsing() {
    let fx = Fixture::with_corpus(CORPUS);
    let parser = fx.parser("parse.sh", ECHO_FILE);
    eprintln!("corpus: {CORPUS} files");

    let (cold_rows, cold) = fx.run(&parser, true);
    assert_eq!(fx.parses(), CORPUS, "the cold run parses every file once");
    assert_eq!(cold_rows.len(), CORPUS);

    let size_before = fx.cache_size();
    let digest_before = fx.cache_digest();
    fx.reset_counter();

    let (warm_rows, warm) = fx.run(&parser, true);

    assert_eq!(
        fx.parses(),
        0,
        "an unchanged tree must not re-run the parser for any file",
    );
    assert_eq!(warm_rows, cold_rows, "the warm run returns the same rows");
    assert_eq!(
        fx.cache_size(),
        size_before,
        "an unchanged tree must not grow the cache",
    );
    assert_eq!(
        fx.cache_digest(),
        digest_before,
        "an unchanged tree must not rewrite the cache",
    );
    let ratio = warm.as_secs_f64() / cold.as_secs_f64();
    assert!(
        ratio <= WARM_RATIO,
        "the warm run must cost at most {:.0}% of the cold one; \
         cold {cold:?}, warm {warm:?} ({:.1}%)",
        WARM_RATIO * 100.0,
        ratio * 100.0,
    );
}

#[test]
fn a_single_changed_file_is_the_only_one_reparsed() {
    let fx = Fixture::with_corpus(40);
    let parser = fx.parser("parse.sh", ECHO_FILE);
    let _ = fx.run(&parser, true);
    fx.reset_counter();

    let changed = fx.file(7);
    fs::write(&changed, payload(7, "v2")).unwrap();
    touch_into_the_future(&changed);

    let (rows, _) = fx.run(&parser, true);

    assert_eq!(fx.parses(), 1, "only the changed file is re-parsed");
    assert!(
        rows.contains(&"Integer(7)|Text(\"v2\")".to_string()),
        "the changed file's new rows are returned: {rows:?}",
    );
    assert_eq!(rows.len(), 40, "every other file keeps its cached row");
}

#[test]
fn a_persisted_run_returns_what_a_non_persisted_run_does() {
    let fx = Fixture::with_corpus(40);
    let parser = fx.parser("parse.sh", ECHO_FILE);

    let (fresh, _) = fx.run(&parser, false);
    let (cold, _) = fx.run(&parser, true);
    let (warm, _) = fx.run(&parser, true);

    assert_eq!(cold, fresh, "a cold persisted run matches an ephemeral one");
    assert_eq!(warm, fresh, "a warm persisted run matches an ephemeral one");
}

#[test]
fn a_deleted_file_drops_out_of_the_cached_rows() {
    let fx = Fixture::with_corpus(40);
    let parser = fx.parser("parse.sh", ECHO_FILE);
    let _ = fx.run(&parser, true);
    fs::remove_file(fx.file(7)).unwrap();
    fx.reset_counter();

    let (rows, _) = fx.run(&parser, true);

    assert_eq!(rows.len(), 39, "the deleted file's row is gone");
    assert_eq!(fx.parses(), 0, "no surviving file is re-parsed");
}

#[test]
fn a_new_file_is_the_only_one_parsed() {
    let fx = Fixture::with_corpus(40);
    let parser = fx.parser("parse.sh", ECHO_FILE);
    let _ = fx.run(&parser, true);
    fx.reset_counter();

    let added = fx.file(40);
    fs::create_dir_all(added.parent().unwrap()).unwrap();
    fs::write(added, payload(40, "v1")).unwrap();
    let (rows, _) = fx.run(&parser, true);

    assert_eq!(fx.parses(), 1, "only the new file is parsed");
    assert_eq!(rows.len(), 41, "its rows join the cached ones");
}

#[test]
fn a_changed_parser_command_invalidates_the_cached_rows() {
    let fx = Fixture::with_corpus(20);
    let _ = fx.run(&fx.parser("parse.sh", ECHO_FILE), true);
    fx.reset_counter();

    // Same tree, different parser: the cached rows describe the old command's
    // output and must not be served for the new one.
    let other = fx.parser("other.sh", "printf '[{\"id\":-1,\"tag\":\"other\"}]'");
    let (rows, _) = fx.run(&other, true);

    assert_eq!(fx.parses(), 20, "every file runs the new parser");
    assert!(
        rows.iter().all(|r| r.ends_with("Text(\"other\")")),
        "the new parser's rows are served, not the cached ones: {rows:?}",
    );
}

#[test]
fn files_inside_the_racy_window_are_hash_confirmed_rather_than_reparsed() {
    // Every file is pinned into the window, so no reuse decision here can be
    // settled by the stat tuple: each one is reused only if its content hash
    // confirms it. A hash that cannot be computed cannot confirm, and the whole
    // corpus goes back to the parser.
    let fx = Fixture::with_corpus(8);
    let parser = fx.parser("parse.sh", ECHO_FILE);
    let pinned: Vec<SystemTime> = (0..8)
        .map(|i| pin_inside_the_racy_window(&fx.file(i)))
        .collect();

    let (cold_rows, _) = fx.run(&parser, true);

    assert_eq!(fx.parses(), 8, "the cold run parses every file once");
    let now = SystemTime::now();
    assert!(
        pinned.iter().all(|mtime| *mtime > now),
        "the cold run must finish before the pinned mtimes, or the corpus is \
         not in the racy window and this test proves nothing",
    );
    fx.reset_counter();

    let (warm_rows, _) = fx.run(&parser, true);

    assert_eq!(
        fx.parses(),
        0,
        "a hash-confirmed file must not reach the parser",
    );
    assert_eq!(warm_rows, cold_rows, "the warm run returns the same rows");
}

#[test]
fn content_that_leaves_the_stat_tuple_untouched_is_caught_by_the_hash() {
    // The one edit the stat tuple cannot see: same size, same mtime, same
    // inode, different bytes. Inside the racy window the content hash is the
    // only thing standing between the query and a stale cached payload.
    let fx = Fixture::with_corpus(8);
    let parser = fx.parser("parse.sh", ECHO_FILE);
    let edited = fx.file(3);
    let pinned = pin_inside_the_racy_window(&edited);

    let (cold_rows, _) = fx.run(&parser, true);
    assert!(cold_rows.contains(&"Integer(3)|Text(\"v1\")".to_string()));
    let before = fs::metadata(&edited).unwrap();
    fx.reset_counter();

    // `payload` is the same length for either tag, so rewriting in place leaves
    // size, inode and device alone; restoring the mtime leaves the rest.
    fs::write(&edited, payload(3, "v2")).unwrap();
    fs::File::open(&edited)
        .unwrap()
        .set_modified(pinned)
        .unwrap();

    let after = fs::metadata(&edited).unwrap();
    assert_eq!(
        after.len(),
        before.len(),
        "the edit must not change the size"
    );
    assert_eq!(
        after.modified().unwrap(),
        before.modified().unwrap(),
        "the edit must not change the mtime",
    );
    assert_eq!(
        after.created().ok(),
        before.created().ok(),
        "the edit must not change the creation time",
    );

    let (rows, _) = fx.run(&parser, true);

    assert_eq!(fx.parses(), 1, "the edited file, and only it, is re-parsed");
    assert!(
        rows.contains(&"Integer(3)|Text(\"v2\")".to_string()),
        "the edited file's new rows are returned, not its cached ones: {rows:?}",
    );
}
