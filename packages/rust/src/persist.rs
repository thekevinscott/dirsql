//! Persistent on-disk SQLite cache (issue #95).
//!
//! Stores the in-memory SQLite database on disk between runs so subsequent
//! startups only re-parse files that have actually changed. Uses the same
//! racy-stat algorithm as `git status`: a per-file `(size, mtime, ctime,
//! inode, dev)` tuple is compared against the cache, with a content-hash
//! confirmation for files whose mtime falls inside the racy window.
//!
//! See `docs/guide/persistence.md` for the user-facing contract.

use rusqlite::{Connection, params};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Table;

/// Sidecar schema version. Bumped on any breaking change to the layout of
/// `_dirsql_files` / `_dirsql_meta`.
pub const SCHEMA_VERSION: &str = "1";

/// Bumped whenever any built-in parser changes its row-shape contract. A
/// mismatch forces a full rebuild.
pub const PARSER_VERSIONS_JSON: &str =
    r#"{"json":"1","jsonl":"1","csv":"1","tsv":"1","toml":"1","yaml":"1","md":"1"}"#;

pub const META_TABLE: &str = "_dirsql_meta";
pub const FILES_TABLE: &str = "_dirsql_files";

pub const META_KEY_SCHEMA_VERSION: &str = "schema_version";
pub const META_KEY_DIRSQL_VERSION: &str = "dirsql_version";
pub const META_KEY_GLOB_CONFIG_HASH: &str = "glob_config_hash";
pub const META_KEY_PARSER_VERSIONS: &str = "parser_versions";
pub const META_KEY_ROOT_CANONICAL: &str = "root_canonical";

/// Filesystem stat tuple used for racy-stat change detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub size: i64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub inode: i64,
    pub dev: i64,
}

impl FileStat {
    pub fn from_metadata(meta: &fs::Metadata) -> Self {
        let mtime_ns = system_time_to_ns(meta.modified().ok());
        let ctime_ns = system_time_to_ns(meta.created().ok());
        let (inode, dev) = inode_dev(meta);
        Self {
            size: meta.len() as i64,
            mtime_ns,
            ctime_ns,
            inode,
            dev,
        }
    }
}

#[cfg(unix)]
fn inode_dev(meta: &fs::Metadata) -> (i64, i64) {
    use std::os::unix::fs::MetadataExt;
    (meta.ino() as i64, meta.dev() as i64)
}

#[cfg(not(unix))]
fn inode_dev(_meta: &fs::Metadata) -> (i64, i64) {
    // Best-effort on non-Unix: zero out inode/dev. Falls back to size+mtime.
    (0, 0)
}

fn system_time_to_ns(t: Option<SystemTime>) -> i64 {
    match t.and_then(|st| st.duration_since(UNIX_EPOCH).ok()) {
        Some(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        None => 0,
    }
}

/// Snapshot timestamp at which the current cache write occurred. Used to
/// gate the racy window: any file whose `mtime_ns >= snapshot_ns` is in the
/// racy window and must be hash-confirmed instead of trusted.
pub fn now_ns() -> i64 {
    system_time_to_ns(Some(SystemTime::now()))
}

/// Per-file row from `_dirsql_files`.
#[derive(Debug, Clone)]
pub struct CachedFile {
    pub rel_path: String,
    pub table_name: String,
    pub stat: FileStat,
    pub content_hash: Option<[u8; 32]>,
    pub snapshot_ns: i64,
}

/// Compute the BLAKE3 content hash of a file.
pub fn hash_file(path: &Path) -> io::Result<[u8; 32]> {
    let bytes = fs::read(path)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

/// Compute the canonical glob-config hash. Includes table name, DDL, glob,
/// and strict flag for every table, plus the ignore list, in a
/// deterministic order. A mismatch against the cached value triggers a
/// full rebuild.
pub fn compute_glob_config_hash(tables: &[Table], ignore: &[String]) -> String {
    let mut entries: BTreeMap<String, (String, String, bool)> = BTreeMap::new();
    for table in tables {
        let name = crate::db::parse_table_name(&table.ddl).unwrap_or_default();
        entries.insert(name, (table.ddl.clone(), table.glob.clone(), table.strict));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"v1\n");
    for (name, (ddl, glob, strict)) in &entries {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(ddl.as_bytes());
        hasher.update(b"\0");
        hasher.update(glob.as_bytes());
        hasher.update(b"\0");
        hasher.update(if *strict { b"1" } else { b"0" });
        hasher.update(b"\n");
    }
    hasher.update(b"--ignore--\n");
    let mut sorted_ignore = ignore.to_vec();
    sorted_ignore.sort();
    for pat in sorted_ignore {
        hasher.update(pat.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().to_hex().to_string()
}

/// Canonicalize the root directory for storage in `_dirsql_meta`. Falls
/// back to the literal path when canonicalization fails (e.g. dangling
/// symlink).
pub fn canonical_root(root: &Path) -> String {
    fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// Resolve the persist path for the cache database.
/// Defaults to `<root>/.dirsql/cache.db`.
pub fn resolve_persist_path(root: &Path, override_path: Option<&Path>) -> PathBuf {
    match override_path {
        Some(p) => p.to_path_buf(),
        None => root.join(".dirsql").join("cache.db"),
    }
}

/// Ensure the directory containing `path` exists. No-op if it already does.
pub fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Create the `_dirsql_meta` and `_dirsql_files` sidecar tables (if they
/// don't already exist).
pub fn create_sidecar_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _dirsql_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS _dirsql_files (
            rel_path     TEXT PRIMARY KEY,
            table_name   TEXT NOT NULL,
            size         INTEGER NOT NULL,
            mtime_ns     INTEGER NOT NULL,
            ctime_ns     INTEGER NOT NULL,
            inode        INTEGER NOT NULL,
            dev          INTEGER NOT NULL,
            content_hash BLOB,
            snapshot_ns  INTEGER NOT NULL
         );",
    )?;
    Ok(())
}

/// Read all `_dirsql_meta` key/value pairs.
pub fn read_meta(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM _dirsql_meta")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (k, v) = row?;
        out.insert(k, v);
    }
    Ok(out)
}

/// Write or replace a single `_dirsql_meta` entry.
pub fn upsert_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO _dirsql_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Replace the entire `_dirsql_meta` contents with the given map.
pub fn write_meta(conn: &Connection, entries: &HashMap<String, String>) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM _dirsql_meta", [])?;
    for (k, v) in entries {
        upsert_meta(conn, k, v)?;
    }
    Ok(())
}

/// Compute the meta map for the current build.
pub fn build_meta(glob_config_hash: &str, canonical_root_str: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        META_KEY_SCHEMA_VERSION.to_string(),
        SCHEMA_VERSION.to_string(),
    );
    m.insert(
        META_KEY_DIRSQL_VERSION.to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    m.insert(
        META_KEY_GLOB_CONFIG_HASH.to_string(),
        glob_config_hash.to_string(),
    );
    m.insert(
        META_KEY_PARSER_VERSIONS.to_string(),
        PARSER_VERSIONS_JSON.to_string(),
    );
    m.insert(
        META_KEY_ROOT_CANONICAL.to_string(),
        canonical_root_str.to_string(),
    );
    m
}

/// Decide whether the cache is reusable. Returns `true` when every
/// expected meta key matches its cached value; `false` when any key is
/// missing or different (which forces a full rebuild).
pub fn meta_is_compatible(
    cached: &HashMap<String, String>,
    expected: &HashMap<String, String>,
) -> bool {
    for (k, v) in expected {
        match cached.get(k) {
            Some(cached_v) if cached_v == v => continue,
            _ => return false,
        }
    }
    true
}

/// Read every `_dirsql_files` row, keyed by relative path.
pub fn read_cached_files(conn: &Connection) -> rusqlite::Result<HashMap<String, CachedFile>> {
    let mut stmt = conn.prepare(
        "SELECT rel_path, table_name, size, mtime_ns, ctime_ns, inode, dev,
                content_hash, snapshot_ns
         FROM _dirsql_files",
    )?;
    let rows = stmt.query_map([], |row| {
        let hash_bytes: Option<Vec<u8>> = row.get(7)?;
        let content_hash = hash_bytes.and_then(|b| {
            if b.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&b);
                Some(arr)
            } else {
                None
            }
        });
        Ok(CachedFile {
            rel_path: row.get(0)?,
            table_name: row.get(1)?,
            stat: FileStat {
                size: row.get(2)?,
                mtime_ns: row.get(3)?,
                ctime_ns: row.get(4)?,
                inode: row.get(5)?,
                dev: row.get(6)?,
            },
            content_hash,
            snapshot_ns: row.get(8)?,
        })
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let cf = row?;
        out.insert(cf.rel_path.clone(), cf);
    }
    Ok(out)
}

/// Write or replace a single file row. `snapshot_ns` is taken at the time
/// of the write.
pub fn upsert_file(
    conn: &Connection,
    rel_path: &str,
    table_name: &str,
    stat: &FileStat,
    content_hash: Option<&[u8; 32]>,
    snapshot_ns: i64,
) -> rusqlite::Result<()> {
    let hash_blob: Option<&[u8]> = content_hash.map(|h| h.as_slice());
    conn.execute(
        "INSERT INTO _dirsql_files
            (rel_path, table_name, size, mtime_ns, ctime_ns, inode, dev,
             content_hash, snapshot_ns)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(rel_path) DO UPDATE SET
            table_name   = excluded.table_name,
            size         = excluded.size,
            mtime_ns     = excluded.mtime_ns,
            ctime_ns     = excluded.ctime_ns,
            inode        = excluded.inode,
            dev          = excluded.dev,
            content_hash = excluded.content_hash,
            snapshot_ns  = excluded.snapshot_ns",
        params![
            rel_path,
            table_name,
            stat.size,
            stat.mtime_ns,
            stat.ctime_ns,
            stat.inode,
            stat.dev,
            hash_blob,
            snapshot_ns,
        ],
    )?;
    Ok(())
}

/// Delete a file's row from `_dirsql_files`.
pub fn delete_file(conn: &Connection, rel_path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM _dirsql_files WHERE rel_path = ?1",
        params![rel_path],
    )?;
    Ok(())
}

/// Drop every user-defined table from a cache database. Used when the
/// reconcile detects an incompatible meta state and we need to wipe the
/// cache before re-ingesting from scratch.
pub fn drop_user_tables(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt =
        conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '_dirsql_%' AND name NOT LIKE 'sqlite_%'")?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);
    for name in names {
        conn.execute(&format!("DROP TABLE IF EXISTS \"{name}\""), [])?;
    }
    conn.execute("DELETE FROM _dirsql_files", [])?;
    Ok(())
}

/// Look up the existing `dirsql_*` tracking column rows for one file. Used
/// on warm start to rebuild the in-memory `file_rows` cache that the
/// watcher's diffing path requires.
pub fn read_rows_for_file(
    conn: &Connection,
    table: &str,
    rel_path: &str,
    user_columns: &[String],
) -> rusqlite::Result<Vec<HashMap<String, crate::db::Value>>> {
    let mut col_list = user_columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    if col_list.is_empty() {
        col_list = "1".to_string();
    }
    let sql = format!(
        "SELECT {col_list} FROM \"{table}\" WHERE _dirsql_file_path = ?1 ORDER BY _dirsql_row_index"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![rel_path], |row| {
        let mut map = HashMap::new();
        for (i, name) in user_columns.iter().enumerate() {
            let v: rusqlite::types::Value = row.get(i)?;
            map.insert(name.clone(), crate::db::Value::from(v));
        }
        Ok(map)
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn glob_config_hash_is_deterministic() {
        let t1 = Table::new("CREATE TABLE a (x TEXT)", "*.json", |_, _| vec![]);
        let t2 = Table::new("CREATE TABLE b (y TEXT)", "*.csv", |_, _| vec![]);
        let h1 = compute_glob_config_hash(
            &[t1.clone(), t2.clone()],
            &["foo".to_string(), "bar".to_string()],
        );
        let h2 = compute_glob_config_hash(&[t2, t1], &["bar".to_string(), "foo".to_string()]);
        assert_eq!(h1, h2, "hash must not depend on input ordering");
    }

    #[test]
    fn glob_config_hash_changes_when_glob_changes() {
        let t1 = Table::new("CREATE TABLE a (x TEXT)", "*.json", |_, _| vec![]);
        let t2 = Table::new("CREATE TABLE a (x TEXT)", "*.csv", |_, _| vec![]);
        let h1 = compute_glob_config_hash(&[t1], &[]);
        let h2 = compute_glob_config_hash(&[t2], &[]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn glob_config_hash_changes_when_strict_changes() {
        let mut t1 = Table::new("CREATE TABLE a (x TEXT)", "*.json", |_, _| vec![]);
        let mut t2 = Table::new("CREATE TABLE a (x TEXT)", "*.json", |_, _| vec![]);
        t2.strict = true;
        let h1 = compute_glob_config_hash(&[t1.clone()], &[]);
        let h2 = compute_glob_config_hash(&[t2], &[]);
        assert_ne!(h1, h2);
        // Sanity: still equal when reset.
        t1.strict = false;
        assert_eq!(h1, compute_glob_config_hash(&[t1], &[]));
    }

    #[test]
    fn meta_is_compatible_passes_when_all_keys_match() {
        let mut a = HashMap::new();
        a.insert("k1".into(), "v1".into());
        a.insert("k2".into(), "v2".into());
        let b = a.clone();
        assert!(meta_is_compatible(&a, &b));
    }

    #[test]
    fn meta_is_compatible_fails_on_mismatch_or_missing() {
        let mut expected = HashMap::new();
        expected.insert("k1".into(), "v1".into());
        let mut cached = HashMap::new();
        cached.insert("k1".into(), "different".into());
        assert!(!meta_is_compatible(&cached, &expected));

        cached.clear();
        assert!(!meta_is_compatible(&cached, &expected));
    }

    #[test]
    fn create_sidecar_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create_sidecar_tables(&conn).unwrap();
        create_sidecar_tables(&conn).unwrap();
    }

    #[test]
    fn upsert_and_read_meta_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        create_sidecar_tables(&conn).unwrap();
        upsert_meta(&conn, "k", "v1").unwrap();
        upsert_meta(&conn, "k", "v2").unwrap();
        let m = read_meta(&conn).unwrap();
        assert_eq!(m.get("k"), Some(&"v2".to_string()));
    }

    #[test]
    fn upsert_and_read_files_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        create_sidecar_tables(&conn).unwrap();
        let stat = FileStat {
            size: 10,
            mtime_ns: 100,
            ctime_ns: 200,
            inode: 1,
            dev: 1,
        };
        let hash = [7u8; 32];
        upsert_file(&conn, "a.csv", "rows", &stat, Some(&hash), 1234).unwrap();
        let files = read_cached_files(&conn).unwrap();
        assert_eq!(files.len(), 1);
        let cf = files.get("a.csv").unwrap();
        assert_eq!(cf.table_name, "rows");
        assert_eq!(cf.stat, stat);
        assert_eq!(cf.content_hash, Some(hash));
        assert_eq!(cf.snapshot_ns, 1234);
    }

    #[test]
    fn delete_file_removes_row() {
        let conn = Connection::open_in_memory().unwrap();
        create_sidecar_tables(&conn).unwrap();
        let stat = FileStat {
            size: 1,
            mtime_ns: 1,
            ctime_ns: 1,
            inode: 1,
            dev: 1,
        };
        upsert_file(&conn, "a.csv", "t", &stat, None, 1).unwrap();
        delete_file(&conn, "a.csv").unwrap();
        assert!(read_cached_files(&conn).unwrap().is_empty());
    }

    #[test]
    fn drop_user_tables_clears_user_data_and_files_index() {
        let conn = Connection::open_in_memory().unwrap();
        create_sidecar_tables(&conn).unwrap();
        conn.execute("CREATE TABLE rows (x TEXT)", []).unwrap();
        conn.execute("INSERT INTO rows (x) VALUES ('a')", [])
            .unwrap();
        let stat = FileStat {
            size: 1,
            mtime_ns: 1,
            ctime_ns: 1,
            inode: 1,
            dev: 1,
        };
        upsert_file(&conn, "a.csv", "rows", &stat, None, 1).unwrap();
        upsert_meta(&conn, "x", "y").unwrap();

        drop_user_tables(&conn).unwrap();

        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'rows'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
        assert!(read_cached_files(&conn).unwrap().is_empty());
        // Meta is preserved by drop_user_tables -- we replace it explicitly.
        let m = read_meta(&conn).unwrap();
        assert_eq!(m.get("x"), Some(&"y".to_string()));
    }

    #[test]
    fn hash_file_returns_blake3_digest() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, b"hello").unwrap();
        let h = hash_file(&p).unwrap();
        let expected = *blake3::hash(b"hello").as_bytes();
        assert_eq!(h, expected);
    }

    #[test]
    fn resolve_persist_path_defaults_to_dirsql_cache_db() {
        let p = resolve_persist_path(Path::new("/tmp/x"), None);
        assert_eq!(p, PathBuf::from("/tmp/x/.dirsql/cache.db"));
    }

    #[test]
    fn resolve_persist_path_honors_override() {
        let p = resolve_persist_path(Path::new("/tmp/x"), Some(Path::new("/var/cache/db")));
        assert_eq!(p, PathBuf::from("/var/cache/db"));
    }

    #[test]
    fn ensure_parent_dir_creates_missing_parents() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a/b/c/file.db");
        ensure_parent_dir(&p).unwrap();
        assert!(p.parent().unwrap().is_dir());
    }

    #[test]
    fn read_rows_for_file_returns_user_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE rows (col TEXT, _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO rows (col, _dirsql_file_path, _dirsql_row_index) VALUES ('a', 'f.csv', 0), ('b', 'f.csv', 1), ('c', 'g.csv', 0)",
            [],
        )
        .unwrap();
        let rows = read_rows_for_file(&conn, "rows", "f.csv", &["col".to_string()]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["col"], crate::db::Value::Text("a".into()));
        assert_eq!(rows[1]["col"], crate::db::Value::Text("b".into()));
    }
}
