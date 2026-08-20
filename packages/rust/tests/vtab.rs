//! Integration tests for the glob-backed path-table virtual table: real temp
//! trees, real files, real SQLite. The inline unit module in `src/vtab.rs`
//! covers only the pure helpers (unit-lint isolation keeps effectful std out
//! of it), so every behavior that depends on the filesystem lives here.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use dirsql::vtab::load_module;
use rusqlite::Connection;
use tempfile::TempDir;

/// Strip every permission bit from `path`, reporting whether the OS actually
/// enforces the result. A privileged process -- root, or anything holding
/// `CAP_DAC_OVERRIDE` -- reads the file regardless, so the unreadable
/// precondition cannot hold there and the caller has nothing to assert.
fn make_unreadable(path: &Path) -> bool {
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
    fs::read(path).is_err()
}

/// A connection with the path-table module registered and one vtab named `t`
/// spanning `glob` under `dir`.
fn open_over(dir: &TempDir, glob: &str) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn).unwrap();
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE t USING dirsql_path('{}', '{}', '', 'gitignore')",
        dir.path().display(),
        glob
    ))
    .unwrap();
    conn
}

/// A connection whose vtab reports paths under `prefix` and skips `ignore`.
fn open_over_with(dir: &TempDir, glob: &str, prefix: &str, ignore: &[&str]) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn).unwrap();
    let mut args = format!(
        "'{}', '{}', '{}', 'gitignore'",
        dir.path().display(),
        glob,
        prefix
    );
    for pattern in ignore {
        args.push_str(&format!(", '{pattern}'"));
    }
    conn.execute_batch(&format!("CREATE VIRTUAL TABLE t USING dirsql_path({args})"))
        .unwrap();
    conn
}

fn column_names(conn: &Connection, sql: &str) -> Vec<String> {
    let stmt = conn.prepare(sql).unwrap();
    stmt.column_names().into_iter().map(String::from).collect()
}

#[test]
fn select_star_returns_exactly_the_seven_stat_columns() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "body").unwrap();
    let conn = open_over(&dir, "**/*");

    assert_eq!(
        column_names(&conn, "SELECT * FROM t"),
        vec!["path", "basename", "dir", "ext", "size", "mtime", "ctime"],
        "content must be HIDDEN and therefore excluded from SELECT *"
    );
}

#[test]
fn content_is_excluded_from_star_but_selectable_by_name() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "hello body").unwrap();
    let conn = open_over(&dir, "**/*");

    let starred = column_names(&conn, "SELECT * FROM t");
    assert!(
        !starred.contains(&"content".to_string()),
        "content leaked into SELECT *: {starred:?}"
    );

    let body: String = conn
        .query_row("SELECT content FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(body, "hello body");
}

#[test]
fn stat_columns_carry_real_values() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("notes")).unwrap();
    fs::write(dir.path().join("notes/todo.md"), "12345").unwrap();
    let conn = open_over(&dir, "**/*");

    let (path, basename, parent, ext, size): (String, String, String, String, i64) = conn
        .query_row("SELECT path, basename, dir, ext, size FROM t", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap();

    assert_eq!(path, "notes/todo.md", "path is relative to the scan root");
    assert_eq!(basename, "todo.md");
    assert_eq!(parent, "notes");
    assert_eq!(ext, "md", "ext carries no leading dot");
    assert_eq!(size, 5);
}

#[test]
fn mtime_and_ctime_are_unix_seconds() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let mtime: i64 = conn
        .query_row("SELECT mtime FROM t", [], |r| r.get(0))
        .unwrap();

    // Sanity bound rather than an exact value: seconds since the epoch, and
    // comfortably after this feature was written.
    assert!(
        mtime > 1_700_000_000,
        "mtime should be Unix seconds, got {mtime}"
    );
}

#[test]
fn dir_is_empty_string_for_root_level_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("top.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let parent: String = conn
        .query_row("SELECT dir FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(parent, "", "root-level files report an empty dir, not NULL");
}

#[test]
fn content_is_null_for_non_utf8_files() {
    let dir = TempDir::new().unwrap();
    let mut f = fs::File::create(dir.path().join("blob.bin")).unwrap();
    f.write_all(&[0xff, 0xfe, 0x00, 0x9f]).unwrap();
    drop(f);
    let conn = open_over(&dir, "**/*");

    let body: Option<String> = conn
        .query_row("SELECT content FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(body, None, "invalid UTF-8 yields NULL, never an error");
}

#[test]
fn unreadable_file_yields_null_content_without_erroring_the_row() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secret.md");
    fs::write(&path, "classified").unwrap();
    if !make_unreadable(&path) {
        eprintln!("skipped: this process bypasses file permission bits");
        return;
    }
    let conn = open_over(&dir, "**/*");

    let (name, body): (String, Option<String>) = conn
        .query_row("SELECT basename, content FROM t", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();

    assert_eq!(name, "secret.md", "the row still appears");
    assert_eq!(body, None, "unreadable content is NULL, not an error");
}

#[test]
fn star_does_not_read_file_bodies() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("locked.md");
    fs::write(&path, "unreadable").unwrap();
    if !make_unreadable(&path) {
        eprintln!("skipped: this process bypasses file permission bits");
        return;
    }
    let conn = open_over(&dir, "**/*");

    // If SELECT * read content eagerly this would surface the permission
    // error; laziness is what keeps the stat columns queryable regardless.
    let name: String = conn
        .query_row("SELECT basename FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "locked.md");
}

#[test]
fn zero_match_glob_yields_zero_rows_not_an_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*.csv");

    let n: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn glob_scopes_the_walk() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("docs/a.md"), "x").unwrap();
    fs::write(dir.path().join("docs/b.csv"), "x").unwrap();
    fs::write(dir.path().join("c.md"), "x").unwrap();
    let conn = open_over(&dir, "docs/**/*.md");

    let mut stmt = conn.prepare("SELECT path FROM t ORDER BY path").unwrap();
    let paths: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(paths, vec!["docs/a.md"]);
}

#[test]
fn reserved_dirsql_directory_is_skipped() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".dirsql")).unwrap();
    fs::write(dir.path().join(".dirsql/cache.db"), "x").unwrap();
    fs::write(dir.path().join("real.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let mut stmt = conn.prepare("SELECT path FROM t").unwrap();
    let paths: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(
        paths,
        vec!["real.md"],
        "the reserved .dirsql/ tree is never surfaced"
    );
}

#[test]
fn directories_are_not_rows() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/a.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let n: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "only files become rows");
}

#[test]
fn writes_are_rejected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let err = conn.execute("DELETE FROM t", []).unwrap_err();
    assert!(
        err.to_string().contains("may not be modified"),
        "read-only is enforced by omitting xUpdate; got: {err}"
    );
}

#[test]
fn joins_against_an_ordinary_table() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    fs::write(dir.path().join("b.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");
    conn.execute_batch(
        "CREATE TABLE tags(name TEXT, tag TEXT);
         INSERT INTO tags VALUES ('a.md', 'keep');",
    )
    .unwrap();

    let tag: String = conn
        .query_row(
            "SELECT tags.tag FROM t JOIN tags ON tags.name = t.basename",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tag, "keep");
}

#[test]
fn reads_are_live_across_statements() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    fs::write(dir.path().join("b.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let before: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, 2);

    fs::remove_file(dir.path().join("b.md")).unwrap();

    let after: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, 1, "the scan happens at query time, not at CREATE");
}

#[test]
fn a_path_prefix_is_prepended_to_the_reported_path() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    let conn = open_over_with(&dir, "**/*", "/elsewhere", &[]);

    let path: String = conn
        .query_row("SELECT path FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(path, "/elsewhere/a.md");
}

#[test]
fn ignore_patterns_skip_matching_files() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules/x.js"), "x").unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    let conn = open_over_with(&dir, "**/*", "", &["node_modules/**"]);

    let paths: Vec<String> = conn
        .prepare("SELECT path FROM t")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(paths, vec!["a.md"]);
}

#[test]
fn a_pattern_naming_an_ignored_directory_still_scans_it() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules/x.js"), "x").unwrap();
    let conn = open_over_with(&dir, "node_modules/**/*", "", &["node_modules/**"]);

    let count: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "skip rules apply below the path you name");
}
