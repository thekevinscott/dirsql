//! Integration tests for the glob-backed path-table virtual table: real temp
//! trees, real files, real SQLite. The inline unit module in `src/vtab.rs`
//! covers only the pure helpers (unit-lint isolation keeps effectful std out
//! of it), so every behavior that depends on the filesystem lives here.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use dirsql::vtab::{StatementScope, load_module};
use rusqlite::Connection;
use tempfile::TempDir;

/// Strip every permission bit from `path`, reporting whether the OS actually
/// enforces the result. A privileged process -- root, or anything holding
/// `CAP_DAC_OVERRIDE` -- reads the file regardless, so the unreadable
/// precondition cannot hold there and the caller has nothing to assert.
#[cfg(unix)]
fn make_unreadable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
    fs::read(path).is_err()
}

#[cfg(not(unix))]
fn make_unreadable(_path: &Path) -> bool {
    false
}

/// A connection with the path-table module registered and one vtab named `t`
/// spanning `glob` under `dir`, plus the scope that ends a statement over it.
fn open_scoped(dir: &TempDir, glob: &str) -> (Connection, Arc<StatementScope>) {
    let conn = Connection::open_in_memory().unwrap();
    let scope = StatementScope::new();
    load_module(&conn, Arc::clone(&scope)).unwrap();
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE t USING dirsql_path('{}', '{}', '', 'gitignore')",
        dir.path().display(),
        glob
    ))
    .unwrap();
    (conn, scope)
}

/// [`open_scoped`] for tests that run one statement.
fn open_over(dir: &TempDir, glob: &str) -> Connection {
    open_scoped(dir, glob).0
}

/// A connection whose vtab reports paths under `prefix` and skips `ignore`.
fn open_over_with(dir: &TempDir, glob: &str, prefix: &str, ignore: &[&str]) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn, StatementScope::new()).unwrap();
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
fn dirsql_directory_is_an_ordinary_dot_directory() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".dirsql")).unwrap();
    fs::write(dir.path().join(".dirsql/cache.db"), "x").unwrap();
    fs::write(dir.path().join("real.md"), "x").unwrap();
    let conn = open_over(&dir, ".dirsql/*");

    let mut stmt = conn.prepare("SELECT path FROM t").unwrap();
    let paths: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(paths, vec![".dirsql/cache.db"]);
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
    let (conn, scope) = open_scoped(&dir, "**/*");

    let before: i64 = conn
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, 2);

    fs::remove_file(dir.path().join("b.md")).unwrap();
    scope.reset();

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
fn a_table_rooted_in_an_ignored_directory_still_scans_it() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules/x.js"), "x").unwrap();
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn, StatementScope::new()).unwrap();
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE t USING dirsql_path('{}', '**/*', 'node_modules', 'gitignore', '**/node_modules/**')",
        dir.path().join("node_modules").display()
    ))
    .unwrap();

    let paths: Vec<String> = conn
        .prepare("SELECT path FROM t")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        paths,
        vec!["node_modules/x.js"],
        "skip rules are judged relative to the scan root, not the reported path"
    );
}

#[test]
fn rowids_count_up_from_zero_in_scan_order() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    fs::write(dir.path().join("b.md"), "x").unwrap();
    fs::write(dir.path().join("c.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");

    let mut stmt = conn
        .prepare("SELECT rowid FROM t ORDER BY basename")
        .unwrap();
    let rowids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rowids, vec![0, 1, 2]);
}

/// The declared scan cost is what keeps SQLite from treating the table as
/// near-infinitely expensive. Left at the default, every join path costs the
/// same and the planner falls back to building an automatic index over the
/// small ordinary table instead of a plain nested loop.
#[test]
fn a_join_against_a_small_table_plans_as_a_plain_scan() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    fs::write(dir.path().join("b.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");
    conn.execute_batch(
        "CREATE TABLE tags(name TEXT, tag TEXT);
         INSERT INTO tags VALUES ('a.md', 'keep');",
    )
    .unwrap();

    let mut stmt = conn
        .prepare("EXPLAIN QUERY PLAN SELECT tags.tag FROM t JOIN tags ON tags.name = t.basename")
        .unwrap();
    let plan: Vec<String> = stmt
        .query_map([], |r| r.get(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        plan.iter().any(|step| step == "SCAN tags"),
        "expected a plain scan of the ordinary table, got {plan:?}"
    );
    assert!(
        !plan.iter().any(|step| step.contains("AUTOMATIC")),
        "the declared cost must keep SQLite from building an automatic index, got {plan:?}"
    );
}

/// Three references to one path table in a statement must not cost three
/// walks. A function that deletes a file while the first arm scans would, with
/// one walk per arm, make the second arm's count come up short.
#[test]
fn a_statement_walks_the_tree_once_however_often_it_reads_the_table() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "x").unwrap();
    fs::write(dir.path().join("b.md"), "x").unwrap();
    let conn = open_over(&dir, "**/*");
    let doomed = dir.path().join("b.md");
    conn.create_scalar_function(
        "unlink_b",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |_| {
            let _ = fs::remove_file(&doomed);
            Ok(0i64)
        },
    )
    .unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT count(*) FROM t WHERE unlink_b() = 0
             UNION ALL SELECT count(*) FROM t
             UNION ALL SELECT count(*) FROM t",
        )
        .unwrap();
    let counts: Vec<i64> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        counts,
        vec![2, 2, 2],
        "every read of the table within one statement sees the one walk's rows"
    );
}

/// A connection with two path-tables, `a` over `glob_a` and `b` over
/// `glob_b`, both under `dir`, plus the scope that ends a statement over them.
fn open_pair_scoped(
    dir: &TempDir,
    glob_a: &str,
    glob_b: &str,
) -> (Connection, Arc<StatementScope>) {
    let conn = Connection::open_in_memory().unwrap();
    let scope = StatementScope::new();
    load_module(&conn, Arc::clone(&scope)).unwrap();
    for (name, glob) in [("a", glob_a), ("b", glob_b)] {
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE {name} USING dirsql_path('{}', '{glob}', '', 'gitignore')",
            dir.path().display(),
        ))
        .unwrap();
    }
    (conn, scope)
}

/// [`open_pair_scoped`] for tests that run one statement.
fn open_pair(dir: &TempDir, glob_a: &str, glob_b: &str) -> Connection {
    open_pair_scoped(dir, glob_a, glob_b).0
}

fn query_plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    stmt.query_map([], |r| r.get(3))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Joining two path-tables on `dir` must not be a nested rescan of the inner
/// glob: `xBestIndex` accepts the equality and the inner side is driven by
/// lookup, which SQLite reports as a non-zero virtual-table index.
#[test]
fn a_join_of_two_path_tables_looks_the_inner_side_up_by_dir() {
    let dir = TempDir::new().unwrap();
    for paper in ["p1", "p2"] {
        fs::create_dir(dir.path().join(paper)).unwrap();
        fs::write(dir.path().join(paper).join("abstract.md"), "x").unwrap();
        fs::write(dir.path().join(paper).join("title.md"), "x").unwrap();
    }
    let conn = open_pair(&dir, "*/abstract.md", "*/title.md");

    let plan = query_plan(&conn, "SELECT a.dir FROM a JOIN b ON b.dir = a.dir");
    assert!(
        plan.iter()
            .any(|step| step.contains("VIRTUAL TABLE INDEX") && !step.contains("INDEX 0:")),
        "expected the inner path-table to use an equality lookup, got {plan:?}"
    );
}

fn texts(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn open_papers_scoped(dir: &TempDir) -> (Connection, Arc<StatementScope>) {
    for paper in ["p1", "p2", "p3"] {
        fs::create_dir(dir.path().join(paper)).unwrap();
        fs::write(dir.path().join(paper).join("abstract.md"), "x").unwrap();
    }
    fs::write(dir.path().join("p1").join("title.md"), "one").unwrap();
    fs::write(dir.path().join("p3").join("title.md"), "three").unwrap();
    open_pair_scoped(dir, "*/abstract.md", "*/title.md")
}

/// [`open_papers_scoped`] for tests that run one statement.
fn open_papers(dir: &TempDir) -> Connection {
    open_papers_scoped(dir).0
}

#[test]
fn a_lookup_join_on_dir_returns_exactly_the_matching_pairs() {
    let dir = TempDir::new().unwrap();
    let conn = open_papers(&dir);

    let titles = texts(
        &conn,
        "SELECT b.content FROM a JOIN b ON b.dir = a.dir ORDER BY a.dir",
    );
    assert_eq!(titles, vec!["one", "three"]);
}

#[test]
fn equality_on_each_lookup_column_finds_the_row() {
    let dir = TempDir::new().unwrap();
    let conn = open_papers(&dir);

    assert_eq!(
        texts(&conn, "SELECT path FROM b WHERE path = 'p3/title.md'"),
        vec!["p3/title.md"]
    );
    assert_eq!(
        texts(
            &conn,
            "SELECT path FROM b WHERE basename = 'title.md' ORDER BY path"
        ),
        vec!["p1/title.md", "p3/title.md"]
    );
    assert_eq!(
        texts(&conn, "SELECT path FROM b WHERE dir = 'p1'"),
        vec!["p1/title.md"]
    );
}

#[test]
fn a_lookup_with_no_match_yields_no_rows() {
    let dir = TempDir::new().unwrap();
    let conn = open_papers(&dir);

    assert!(texts(&conn, "SELECT path FROM b WHERE dir = 'p2'").is_empty());
}

#[test]
fn a_non_text_or_null_lookup_key_matches_nothing() {
    let dir = TempDir::new().unwrap();
    let conn = open_papers(&dir);

    assert!(texts(&conn, "SELECT path FROM b WHERE basename = 5").is_empty());
    assert!(texts(&conn, "SELECT path FROM b WHERE dir = NULL").is_empty());
}

#[test]
fn rowids_are_the_same_under_lookup_as_under_a_scan() {
    let dir = TempDir::new().unwrap();
    let conn = open_papers(&dir);

    let scanned = texts(&conn, "SELECT rowid || ':' || path FROM b ORDER BY rowid");
    let looked_up = texts(&conn, "SELECT rowid || ':' || path FROM b WHERE dir = 'p3'");
    assert_eq!(scanned, vec!["0:p1/title.md", "1:p3/title.md"]);
    assert_eq!(looked_up, vec!["1:p3/title.md"]);
}

#[test]
fn a_lookup_join_sees_files_created_after_the_previous_statement() {
    let dir = TempDir::new().unwrap();
    let (conn, scope) = open_papers_scoped(&dir);
    let before = texts(
        &conn,
        "SELECT a.dir FROM a JOIN b ON b.dir = a.dir ORDER BY a.dir",
    );

    fs::write(dir.path().join("p2").join("title.md"), "two").unwrap();
    scope.reset();

    let after = texts(
        &conn,
        "SELECT a.dir FROM a JOIN b ON b.dir = a.dir ORDER BY a.dir",
    );
    assert_eq!(before, vec!["p1", "p3"]);
    assert_eq!(after, vec!["p1", "p2", "p3"]);
}
