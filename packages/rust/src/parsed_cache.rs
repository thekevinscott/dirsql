//! The persistent row cache behind a parsed path-table.
//!
//! A parsed path-table ([`crate::parsed_vtab`]) materializes its rows by
//! running the `on-file` parser once per matched file. That is the expensive
//! step `--persist` exists to skip: with a cache, a file whose stat tuple has
//! not moved serves the payload the parser produced last time and the process
//! is never spawned.
//!
//! The cache lives in the same SQLite file the declared tables use
//! (`_dirsql_parsed_rows`) and is keyed by [`table_key`] — the identity of the
//! *table*, not of the query, so two path-tables over the same tree with
//! different parsers never read each other's rows.
//!
//! Staleness is decided by [`crate::persist::is_trusted`], the same racy-stat
//! rule the declared-table reconcile uses.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::persist::{CachedFile, FILE_COLUMNS, FileStat, is_trusted, read_cached_file_row};

/// A cached parse: the file identity the payload was produced from, and the
/// payload itself (the parser's stdout, verbatim).
#[derive(Debug, Clone)]
pub struct CachedParse {
    pub rel_path: String,
    pub stat: FileStat,
    pub content_hash: Option<[u8; 32]>,
    pub snapshot_ns: i64,
    pub payload: String,
}

impl From<(CachedFile, String)> for CachedParse {
    fn from((file, payload): (CachedFile, String)) -> Self {
        Self {
            rel_path: file.rel_path,
            stat: file.stat,
            content_hash: file.content_hash,
            snapshot_ns: file.snapshot_ns,
            payload,
        }
    }
}

/// One file's parse, as handed to [`RowCache::commit`].
pub struct Entry<'a> {
    pub rel_path: &'a str,
    pub stat: &'a FileStat,
    pub content_hash: Option<[u8; 32]>,
    pub snapshot_ns: i64,
    pub payload: &'a str,
}

/// The cache as the vtab sees it: read what a prior run left, record what this
/// one learned. A trait so the reuse logic can be unit-tested against a double
/// instead of a database.
pub trait RowCache {
    /// Every cached parse for this table, keyed by root-relative path.
    fn read(&self) -> rusqlite::Result<HashMap<String, CachedParse>>;

    /// Record `writes` and forget `deletes`, together. Callers skip this
    /// entirely when both are empty, which is what leaves an unchanged run's
    /// cache file byte-for-byte alone.
    fn commit(&self, writes: &[Entry<'_>], deletes: &[&str]) -> rusqlite::Result<()>;
}

/// The identity of a parsed path-table: everything that decides what its rows
/// are. A change to any of it (a different tree, glob, parser, or dirsql build)
/// yields a different key, so stale rows are never served — no invalidation
/// pass required.
pub fn table_key(root: &Path, glob: &str, command: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [
        crate::persist::SCHEMA_VERSION,
        env!("CARGO_PKG_VERSION"),
        &root.to_string_lossy(),
        glob,
        command,
    ] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

/// Create the `_dirsql_parsed_rows` table if it is not already there. Called by
/// [`crate::persist::create_sidecar_tables`], which owns the cache's schema as
/// a whole.
pub fn create_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _dirsql_parsed_rows (
            table_key    TEXT NOT NULL,
            rel_path     TEXT NOT NULL,
            size         INTEGER NOT NULL,
            mtime_ns     INTEGER NOT NULL,
            ctime_ns     INTEGER NOT NULL,
            inode        INTEGER NOT NULL,
            dev          INTEGER NOT NULL,
            content_hash BLOB,
            snapshot_ns  INTEGER NOT NULL,
            payload      TEXT NOT NULL,
            PRIMARY KEY (table_key, rel_path)
         );",
    )
}

/// Forget every cached parse. Used when the whole cache is being discarded.
pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM _dirsql_parsed_rows", [])?;
    Ok(())
}

/// Whether a cached parse can be served for the file `live` describes.
pub fn is_fresh(
    cached: Option<&CachedParse>,
    live: &FileStat,
    hash: impl FnOnce() -> Option<[u8; 32]>,
) -> bool {
    cached.is_some_and(|c| is_trusted(&c.stat, c.content_hash.as_ref(), c.snapshot_ns, live, hash))
}

/// One parsed path-table's slice of the cache, over a connection of its own.
/// The owning connection holds the same file open in WAL mode, which is what
/// makes the concurrent read-and-write safe; the busy timeout covers the moment
/// a checkpoint holds the write lock.
pub struct SqliteRowCache {
    conn: Connection,
    table_key: String,
}

impl SqliteRowCache {
    pub fn open(path: &Path, table_key: String) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        Ok(Self { conn, table_key })
    }
}

impl RowCache for SqliteRowCache {
    fn read(&self) -> rusqlite::Result<HashMap<String, CachedParse>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FILE_COLUMNS}, payload FROM _dirsql_parsed_rows WHERE table_key = ?1"
        ))?;
        let rows = stmt.query_map(params![self.table_key], |row| {
            let file = read_cached_file_row(row, &self.table_key)?;
            let payload: String = row.get(8)?;
            Ok(CachedParse::from((file, payload)))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let parse = row?;
            out.insert(parse.rel_path.clone(), parse);
        }
        Ok(out)
    }

    fn commit(&self, writes: &[Entry<'_>], deletes: &[&str]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for entry in writes {
            let hash_blob: Option<&[u8]> = entry.content_hash.as_ref().map(|h| h.as_slice());
            self.conn.execute(
                "INSERT INTO _dirsql_parsed_rows
                    (table_key, rel_path, size, mtime_ns, ctime_ns, inode, dev,
                     content_hash, snapshot_ns, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(table_key, rel_path) DO UPDATE SET
                    size         = excluded.size,
                    mtime_ns     = excluded.mtime_ns,
                    ctime_ns     = excluded.ctime_ns,
                    inode        = excluded.inode,
                    dev          = excluded.dev,
                    content_hash = excluded.content_hash,
                    snapshot_ns  = excluded.snapshot_ns,
                    payload      = excluded.payload",
                params![
                    self.table_key,
                    entry.rel_path,
                    entry.stat.size,
                    entry.stat.mtime_ns,
                    entry.stat.ctime_ns,
                    entry.stat.inode,
                    entry.stat.dev,
                    hash_blob,
                    entry.snapshot_ns,
                    entry.payload,
                ],
            )?;
        }
        for rel_path in deletes {
            self.conn.execute(
                "DELETE FROM _dirsql_parsed_rows WHERE table_key = ?1 AND rel_path = ?2",
                params![self.table_key, rel_path],
            )?;
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(size: i64, mtime_ns: i64) -> FileStat {
        FileStat {
            size,
            mtime_ns,
            ctime_ns: 5,
            inode: 9,
            dev: 3,
        }
    }

    /// A cache over an in-memory database with this module's table in it.
    fn cache(table_key: &str) -> SqliteRowCache {
        let cache = empty(table_key);
        create_table(&cache.conn).unwrap();
        cache
    }

    /// A cache over a database with no tables at all, for the error paths.
    fn empty(table_key: &str) -> SqliteRowCache {
        SqliteRowCache {
            conn: Connection::open_in_memory().unwrap(),
            table_key: table_key.to_string(),
        }
    }

    fn entry<'a>(rel_path: &'a str, stat: &'a FileStat, payload: &'a str) -> Entry<'a> {
        Entry {
            rel_path,
            stat,
            content_hash: None,
            snapshot_ns: 1,
            payload,
        }
    }

    #[test]
    fn table_key_is_stable_for_the_same_table() {
        let a = table_key(Path::new("/data"), "**/*.json", "cat {path}");
        let b = table_key(Path::new("/data"), "**/*.json", "cat {path}");
        assert_eq!(a, b);
    }

    #[test]
    fn table_key_separates_root_glob_and_parser() {
        let base = table_key(Path::new("/data"), "**/*.json", "cat {path}");
        assert_ne!(
            base,
            table_key(Path::new("/other"), "**/*.json", "cat {path}")
        );
        assert_ne!(base, table_key(Path::new("/data"), "**/*.md", "cat {path}"));
        assert_ne!(
            base,
            table_key(Path::new("/data"), "**/*.json", "jq . {path}")
        );
    }

    #[test]
    fn create_table_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create_table(&conn).unwrap();
        create_table(&conn).unwrap();
    }

    #[test]
    fn commit_and_read_round_trip() {
        let cache = cache("k");
        let stat = stat(10, 100);
        cache
            .commit(
                &[Entry {
                    rel_path: "a.json",
                    stat: &stat,
                    content_hash: Some([4u8; 32]),
                    snapshot_ns: 200,
                    payload: "[{}]",
                }],
                &[],
            )
            .unwrap();

        let cached = cache.read().unwrap();
        let parse = cached.get("a.json").unwrap();
        assert_eq!(parse.payload, "[{}]");
        assert_eq!(parse.stat, stat);
        assert_eq!(parse.content_hash, Some([4u8; 32]));
        assert_eq!(parse.snapshot_ns, 200);
        assert_eq!(parse.rel_path, "a.json");
    }

    #[test]
    fn commit_replaces_an_earlier_parse_of_the_same_file() {
        let cache = cache("k");
        let (old, new) = (stat(1, 1), stat(2, 2));
        cache.commit(&[entry("a.json", &old, "[1]")], &[]).unwrap();
        cache.commit(&[entry("a.json", &new, "[2]")], &[]).unwrap();

        let cached = cache.read().unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached["a.json"].payload, "[2]");
        assert_eq!(cached["a.json"].stat, new);
    }

    #[test]
    fn commit_deletes_the_named_files_only() {
        let cache = cache("k");
        let stat = stat(1, 1);
        cache
            .commit(
                &[entry("a.json", &stat, "[1]"), entry("b.json", &stat, "[2]")],
                &[],
            )
            .unwrap();

        cache.commit(&[], &["a.json"]).unwrap();

        let cached = cache.read().unwrap();
        assert_eq!(cached.len(), 1);
        assert!(cached.contains_key("b.json"));
    }

    #[test]
    fn read_is_scoped_to_one_table_key() {
        let mine = cache("k1");
        let stat = stat(1, 1);
        mine.commit(&[entry("a.json", &stat, "[1]")], &[]).unwrap();

        // A second table over the same database and the same file path.
        let theirs = SqliteRowCache {
            conn: Connection::open_in_memory().unwrap(),
            table_key: "k2".to_string(),
        };
        create_table(&theirs.conn).unwrap();
        theirs
            .commit(&[entry("a.json", &stat, "[2]")], &[])
            .unwrap();

        assert_eq!(mine.read().unwrap()["a.json"].payload, "[1]");
        assert_eq!(theirs.read().unwrap()["a.json"].payload, "[2]");
    }

    #[test]
    fn read_drops_a_malformed_content_hash() {
        let cache = cache("k");
        cache
            .conn
            .execute(
                "INSERT INTO _dirsql_parsed_rows
                    (table_key, rel_path, size, mtime_ns, ctime_ns, inode, dev,
                     content_hash, snapshot_ns, payload)
                 VALUES ('k', 'a.json', 0, 0, 0, 0, 0, ?1, 0, '[]')",
                params![&[1u8, 2][..]],
            )
            .unwrap();
        assert_eq!(cache.read().unwrap()["a.json"].content_hash, None);
    }

    #[test]
    fn clear_forgets_every_cached_parse() {
        let cache = cache("k");
        let stat = stat(1, 1);
        cache.commit(&[entry("a.json", &stat, "[1]")], &[]).unwrap();

        clear(&cache.conn).unwrap();

        assert!(cache.read().unwrap().is_empty());
    }

    #[test]
    fn read_propagates_a_missing_table_error() {
        let err = empty("k").read().unwrap_err();
        assert!(err.to_string().contains("no such table"), "got: {err}");
    }

    #[test]
    fn commit_propagates_a_write_error() {
        let stat = stat(1, 1);
        let err = empty("k")
            .commit(&[entry("a.json", &stat, "[]")], &[])
            .unwrap_err();
        assert!(err.to_string().contains("no such table"), "got: {err}");
    }

    #[test]
    fn commit_propagates_a_delete_error() {
        let err = empty("k").commit(&[], &["a.json"]).unwrap_err();
        assert!(err.to_string().contains("no such table"), "got: {err}");
    }

    #[test]
    fn clear_propagates_a_missing_table_error() {
        let conn = Connection::open_in_memory().unwrap();
        let err = clear(&conn).unwrap_err();
        assert!(err.to_string().contains("no such table"), "got: {err}");
    }

    fn parse(stat: FileStat, hash: Option<[u8; 32]>, snapshot_ns: i64) -> CachedParse {
        CachedParse {
            rel_path: "a.json".to_string(),
            stat,
            content_hash: hash,
            snapshot_ns,
            payload: "[]".to_string(),
        }
    }

    #[test]
    fn is_fresh_is_false_without_a_cached_parse() {
        assert!(!is_fresh(None, &stat(1, 1), || None));
    }

    #[test]
    fn is_fresh_trusts_an_unchanged_stat_outside_the_racy_window() {
        let cached = parse(stat(1, 100), None, 200);
        assert!(is_fresh(Some(&cached), &stat(1, 100), || {
            panic!("must not hash outside the racy window")
        }));
    }

    #[test]
    fn is_fresh_is_false_for_a_changed_stat() {
        let cached = parse(stat(1, 100), None, 200);
        assert!(!is_fresh(Some(&cached), &stat(2, 100), || None));
    }

    #[test]
    fn is_fresh_hash_confirms_inside_the_racy_window() {
        let hash = [7u8; 32];
        let cached = parse(stat(1, 100), Some(hash), 50);
        assert!(is_fresh(Some(&cached), &stat(1, 100), || Some(hash)));
        assert!(!is_fresh(Some(&cached), &stat(1, 100), || Some([8u8; 32])));
    }

    #[test]
    fn open_reports_an_unopenable_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("no-such-dir").join("cache.db");
        assert!(SqliteRowCache::open(&missing, "k".to_string()).is_err());
    }

    #[test]
    fn open_yields_a_usable_cache() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cache.db");
        let cache = SqliteRowCache::open(&path, "k".to_string()).unwrap();
        create_table(&cache.conn).unwrap();
        let stat = stat(1, 1);
        cache.commit(&[entry("a.json", &stat, "[1]")], &[]).unwrap();
        assert_eq!(cache.read().unwrap().len(), 1);
    }
}
