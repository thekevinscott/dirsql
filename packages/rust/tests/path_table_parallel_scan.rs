//! Two path-tables named by one statement walk their trees at the same time,
//! the way a shell pipeline runs two `find`s at once. Real filesystem, real
//! SQLite, SDK public API. Its own binary, so no sibling test competes for
//! the cores the overlap needs.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use dirsql::DirSQL;
use tempfile::TempDir;

const DIRS: usize = 50;
const PATTERNS: usize = 100;
const ROUNDS: usize = 4;

/// A tree whose walk costs CPU rather than disk: every directory carries a
/// `.gitignore` the walker compiles on entry (`.git` puts them in force), so a
/// scan takes the same time whether or not the page cache already holds the
/// tree. Each directory nests in the one before, so no one walk has two
/// directories to read at once and only walking both trees together can
/// overlap.
fn tree(root: &Path, name: &str) {
    fs::create_dir_all(root.join(".git")).unwrap();
    let ignore: String = (0..PATTERNS)
        .map(|i| format!("build-{i}/**/*.tmp\n"))
        .collect();
    let mut dir = root.join(name);
    for _ in 0..DIRS {
        dir.push("d");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".gitignore"), &ignore).unwrap();
        fs::write(dir.join("f.txt"), "x").unwrap();
    }
}

fn timed(db: &DirSQL, sql: &str) -> Duration {
    let started = Instant::now();
    db.query(sql).unwrap();
    started.elapsed()
}

/// The fastest of [`ROUNDS`] interleaved runs of each statement: the floor
/// is the honest cost, and a busy box only ever adds to it.
fn floors(db: &DirSQL) -> (Duration, Duration, Duration) {
    let mut a = Duration::MAX;
    let mut b = Duration::MAX;
    let mut both = Duration::MAX;
    for _ in 0..ROUNDS {
        a = a.min(timed(db, "SELECT count(*) FROM './a/**/*.txt'"));
        b = b.min(timed(db, "SELECT count(*) FROM './b/**/*.txt'"));
        both = both.min(timed(
            db,
            "SELECT count(*) FROM './a/**/*.txt' x JOIN './b/**/*.txt' y ON y.path = x.path",
        ));
    }
    (a, b, both)
}

#[test]
fn two_path_tables_in_one_statement_walk_their_trees_at_the_same_time() {
    let root = TempDir::new().unwrap();
    tree(root.path(), "a");
    tree(root.path(), "b");
    let db = DirSQL::new(root.path(), Vec::new()).unwrap();

    let (a, b, both) = floors(&db);

    let serial = a + b;
    assert!(
        both < serial.mul_f64(0.75),
        "a {a:?} + b {b:?} = {serial:?} walked one after the other; \
         the statement over both took {both:?}, so the walks did not overlap"
    );
}
