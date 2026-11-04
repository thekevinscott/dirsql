//! A tree, the instance over it, and the readings the freshness tests assert:
//! a declared table must read exactly what a fresh scan of the disk reads.

#![allow(dead_code)]

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tempfile::TempDir;

/// `recs` is a config table, so its hook runs once over every matched file.
const CONFIG: &str = r#"
[[table]]
name    = "recs"
glob    = "**/*.json"
on-file = "cat"
ddl     = "CREATE TABLE recs (k TEXT, v TEXT)"
"#;

pub struct Tree {
    dir: TempDir,
}

impl Tree {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".dirsql.toml"), CONFIG).unwrap();
        Self { dir }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root().join(rel)
    }

    /// The instance under test: `notes`, a per-file table over `**/*.txt`,
    /// plus the config's `recs`.
    pub fn open(&self) -> DirSQL {
        DirSQL::builder()
            .root(self.root())
            .config(self.path(".dirsql.toml"))
            .table(notes_table(self.root()))
            .build()
            .unwrap()
    }

    pub fn write(&self, rel: &str, content: &str) {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    /// Write a sibling outside every glob, then rename it over `rel`.
    pub fn save_atomically(&self, rel: &str, content: &str) {
        let staged = self.path(&format!("{rel}.stage"));
        if let Some(parent) = staged.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&staged, content).unwrap();
        fs::rename(staged, self.path(rel)).unwrap();
    }

    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.path(rel)).unwrap();
    }

    pub fn rename(&self, from: &str, to: &str) {
        let to = self.path(to);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::rename(self.path(from), to).unwrap();
    }

    pub fn times(&self, rel: &str) -> (SystemTime, SystemTime) {
        let meta = fs::metadata(self.path(rel)).unwrap();
        (meta.accessed().unwrap(), meta.modified().unwrap())
    }

    /// `touch -r`: put back the access and modification times `times` holds.
    pub fn restore_times(&self, rel: &str, times: (SystemTime, SystemTime)) {
        let file = fs::File::options()
            .write(true)
            .open(self.path(rel))
            .unwrap();
        file.set_times(
            fs::FileTimes::new()
                .set_accessed(times.0)
                .set_modified(times.1),
        )
        .unwrap();
    }

    /// Point the symlink `rel` at `target`, replacing any link already there.
    pub fn link(&self, rel: &str, target: &str) {
        let link = self.path(rel);
        if fs::symlink_metadata(&link).is_ok() {
            fs::remove_file(&link).unwrap();
        }
        symlink_file(&self.path(target), &link);
    }
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link).unwrap();
}

/// One row per non-empty line, keyed by the file's root-relative path.
pub fn notes_table(root: &Path) -> Table {
    let roots = [
        root.to_path_buf(),
        root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
    ];
    Table::new(
        "notes",
        "CREATE TABLE notes (file TEXT, line TEXT)",
        "**/*.txt",
        move |path| {
            let path = Path::new(path);
            let rel = roots
                .iter()
                .find_map(|root| path.strip_prefix(root).ok())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(path).unwrap_or_default();
            content
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| {
                    HashMap::from([
                        ("file".to_string(), Value::Text(rel.clone())),
                        ("line".to_string(), Value::Text(line.to_string())),
                    ])
                })
                .collect::<Vec<Row>>()
        },
    )
}

/// Every declared table's rows, sorted.
#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub notes: Vec<(String, String)>,
    pub recs: Vec<(String, String)>,
}

pub fn snapshot(db: &DirSQL) -> Snapshot {
    Snapshot {
        notes: pairs(db, "SELECT file AS a, line AS b FROM notes ORDER BY a, b"),
        recs: pairs(db, "SELECT k AS a, v AS b FROM recs ORDER BY a, b"),
    }
}

fn pairs(db: &DirSQL, sql: &str) -> Vec<(String, String)> {
    db.query(sql)
        .unwrap()
        .iter()
        .map(|row| (text(&row["a"]), text(&row["b"])))
        .collect()
}

pub fn text(value: &Value) -> String {
    match value {
        Value::Text(s) => s.clone(),
        Value::Null => "<null>".to_string(),
        other => format!("{other:?}"),
    }
}

/// The `notes` lines read for `file`, in line order.
pub fn lines_of(db: &DirSQL, file: &str) -> Vec<String> {
    db.query(&format!(
        "SELECT line FROM notes WHERE file = '{file}' ORDER BY line"
    ))
    .unwrap()
    .iter()
    .map(|row| text(&row["line"]))
    .collect()
}

/// The `recs` values read for key `k`.
pub fn recs_of(db: &DirSQL, k: &str) -> Vec<String> {
    db.query(&format!("SELECT v FROM recs WHERE k = '{k}' ORDER BY v"))
        .unwrap()
        .iter()
        .map(|row| text(&row["v"]))
        .collect()
}

/// The oracle: what a fresh scan of the disk reads right now.
pub fn assert_matches_fresh_scan(db: &DirSQL, tree: &Tree, context: &str) {
    let fresh = tree.open();
    assert_eq!(
        snapshot(db),
        snapshot(&fresh),
        "the long-lived instance disagrees with a fresh scan after {context}"
    );
}

/// A small deterministic generator, so a failing sequence replays.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}
