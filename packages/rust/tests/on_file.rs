//! Integration tests for the `on-file` per-table command event.
//!
//! These build a `DirSQL` from a real `.dirsql.toml` whose table declares an
//! `on-file` command, over real temp files, and assert the parsed rows appear
//! via `db.query(...)`. They exercise the effectful spawn path (kept out of
//! colocated unit tests by the Rust isolation rule).
//!
//! Unix-only: the fixtures shell out to `sh`/`cat`. The Rust CI test job runs
//! on Linux.
#![cfg(unix)]

use std::fs;
use std::sync::{Arc, Mutex};

use dirsql::{DirSQL, Table, Value};
use tempfile::TempDir;

/// An `on-file` command that reads the matched file (a JSON array of row
/// objects) and echoes it back as the payload produces one row per array
/// element, with the fields promoted to columns.
#[test]
fn on_file_rows_appear_in_query_results() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE papers (paper_id TEXT, title TEXT, basename TEXT)"
glob = "**/meta.json"
on-file = "cat {path}"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("p1")).unwrap();
    fs::write(
        root.path().join("p1").join("meta.json"),
        r#"[{"paper_id":"a","title":"First","basename":"meta.json"},{"paper_id":"b","title":"Second","basename":"meta.json"}]"#,
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db
        .query("SELECT paper_id, title, basename FROM papers ORDER BY paper_id")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["paper_id"], Value::Text("a".into()));
    assert_eq!(rows[0]["title"], Value::Text("First".into()));
    assert_eq!(rows[0]["basename"], Value::Text("meta.json".into()));
    assert_eq!(rows[1]["paper_id"], Value::Text("b".into()));
    assert_eq!(rows[1]["title"], Value::Text("Second".into()));
}

/// `{abspath}` is no longer a recognized `on-file` token: a template
/// referencing it receives the literal string `{abspath}` (unknown tokens are
/// left literal). The helper echoes its second argument into column `q`, so a
/// substituted `{abspath}` would surface the absolute path; instead `q` is the
/// literal `{abspath}`.
#[test]
fn on_file_abspath_token_is_no_longer_substituted() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("echo_args.sh"),
        "#!/bin/sh\nprintf '[{\"q\":\"%s\"}]' \"$2\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (q TEXT)"
glob = "*.json"
on-file = "sh echo_args.sh {path} {abspath}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), "ignored\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT q FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["q"], Value::Text("{abspath}".into()));
}

/// Interpolation is the only channel for the path: a template that omits
/// `{path}` no longer has it appended, so `on-file = "cat"` runs `cat` with no
/// file (its stdin is null), producing no payload and therefore no rows.
#[test]
fn on_file_omitting_path_no_longer_appends_it() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
on-file = "cat"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), r#"[{"name":"widget"}]"#).unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert!(
        rows.is_empty(),
        "a `{{path}}`-less template must not receive the path, got {rows:?}"
    );
}

/// `{path}` interpolates the matched file's **absolute** path, so an `on-file`
/// script receives a self-sufficient argument that resolves from any cwd. The
/// helper exits non-zero unless its argument is absolute; only then does it
/// `cat` the file. Rows landing proves the script saw an absolute `{path}`.
#[test]
fn on_file_receives_absolute_path() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("abscheck.sh"),
        "#!/bin/sh\ncase \"$1\" in /*) cat \"$1\" ;; *) exit 1 ;; esac\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
on-file = "sh abscheck.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), r#"[{"name":"widget"}]"#).unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(
        rows.len(),
        1,
        "an absolute `{{path}}` must pass the /*-guard and let the script cat the file"
    );
    assert_eq!(rows[0]["name"], Value::Text("widget".into()));
}

/// When the index root differs from the config file's directory (here via an
/// explicit `.root(...)`, since #540 removed the config `root` key), the hook
/// still runs with cwd = the config dir, so a root-relative `{path}` would not
/// resolve. The absolute `{path}` does: the script `cat`s the file from a cwd
/// that is not the index root and rows land.
#[test]
fn on_file_absolute_path_resolves_when_root_differs_from_config_dir() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("abscheck.sh"),
        "#!/bin/sh\ncase \"$1\" in /*) cat \"$1\" ;; *) exit 1 ;; esac\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "**/meta.json"
on-file = "sh abscheck.sh {path}"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(
        root.path().join("data").join("meta.json"),
        r#"[{"name":"widget"}]"#,
    )
    .unwrap();

    // Index root is `data/`; the config (and `abscheck.sh`) live in the parent,
    // so the hook's cwd (the config dir) is not the index root.
    let db = DirSQL::builder()
        .root(root.path().join("data"))
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("widget".into()));
}

/// A file whose command exits non-zero is skipped; the other file's rows are
/// still present and the scan does not error. The command is a helper script
/// (kept out of the TOML to sidestep nested-quote parsing): a file containing
/// `BOOM` makes it exit non-zero, otherwise it emits a one-row JSON array.
#[test]
fn a_failing_command_skips_only_that_file() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q BOOM \"$1\"; then exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    // The scan must succeed despite one file's command failing.
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();

    // Only the good file contributed a row; the bad file was skipped.
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("ok".into()));
}

/// Output that is not a JSON array of objects also isolates to a skip, without
/// aborting the scan.
#[test]
fn malformed_output_skips_only_that_file() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q GOOD \"$1\"; then printf '[{\"name\":\"ok\"}]'; else printf 'not json'; fi\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("good.txt"), "GOOD\n").unwrap();
    fs::write(root.path().join("junk.txt"), "whatever\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("ok".into()));
}

/// Under the default 30-second timeout this command would finish; the skip
/// proves the configured 1-second `hook-timeout` bounded the run.
#[test]
fn on_file_exceeding_configured_timeout_skips_the_file() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("slow.sh"),
        "#!/bin/sh\nsleep 2\nprintf '[{\"name\":\"late\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
hook-timeout = 1

[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh slow.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x\n").unwrap();

    // The scan must succeed; the timed-out file contributes no rows.
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert!(
        rows.is_empty(),
        "a file whose on-file run exceeds `hook-timeout = 1` must be skipped, got {rows:?}"
    );
}

/// Proves `hook-timeout` is read as seconds: `hook-timeout = 5` with a
/// 2-second command lands rows.
#[test]
fn on_file_within_generous_configured_timeout_lands_rows() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("slowish.sh"),
        "#!/bin/sh\nsleep 2\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
hook-timeout = 5

[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh slowish.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("ok".into()));
}

/// A row that fails strict normalization is the hook's mistake, not the
/// database's, so it costs that file and no other. Before dirsql#714 the bare
/// `?` on `normalize_row` aborted the whole scan, and the well-formed file's
/// rows were lost with it.
#[test]
fn a_strict_violation_skips_only_that_file() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("gen.sh"),
        "#!/bin/sh\nif grep -q BAD \"$1\"; then printf '[{\"nope\":1}]'; else printf '[{\"name\":\"ok\"}]'; fi\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
strict = true
on-file = "sh gen.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a_good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("z_bad.txt"), "BAD\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .expect("one bad row must not fail the build");
    let rows = db.query("SELECT name FROM items").unwrap();

    assert_eq!(rows.len(), 1, "the good file's row must survive: {rows:?}");
    assert_eq!(rows[0]["name"], Value::Text("ok".into()));
}

/// A scan attempts every matched file. One hook failure is that file's
/// problem, so it must not stop the files after it from being tried.
#[test]
fn build_attempts_every_matched_file_even_after_one_fails() {
    let dir = TempDir::new().unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(dir.path().join(name), "x").unwrap();
    }
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = Arc::clone(&seen);

    let built = DirSQL::new(
        dir.path(),
        vec![Table::try_new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            move |path| {
                recorder.lock().unwrap().push(path.to_string());
                Err("boom".into())
            },
        )],
    );

    assert!(
        built.is_ok(),
        "per-file failures no longer cancel the scan: {:?}",
        built.err()
    );
    // One guard: `Mutex` is not reentrant, so locking twice in the same
    // assert expression deadlocks rather than failing.
    let attempted = seen.lock().unwrap();
    assert_eq!(
        attempted.len(),
        2,
        "every matched file should be attempted, got: {attempted:?}"
    );
}

/// Reporting only the first failure hides the rest until it is fixed and the
/// scan re-run, one file at a time.
#[test]
fn build_reports_every_failing_file_not_only_the_first() {
    let dir = TempDir::new().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(dir.path().join(name), "x").unwrap();
    }

    let db = DirSQL::new(
        dir.path(),
        vec![Table::try_new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |path| Err(format!("boom for {path}").into()),
        )],
    )
    .expect("per-file failures no longer cancel the scan");

    let reported = db.scan_failures();
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            reported.iter().any(|f| f.path.contains(name)),
            "{name} missing from: {reported:?}"
        );
    }
}
