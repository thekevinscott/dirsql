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

use dirsql::{DirSQL, DirSqlError, Table, Value};
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
name = "papers"
ddl = "CREATE TABLE papers (paper_id TEXT, title TEXT, basename TEXT)"
glob = "**/meta.json"
on-file = "cat"
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

/// `{abspath}` is not a recognized `on-file` token: a template referencing it
/// receives the literal string `{abspath}` (unknown tokens are left literal).
/// The helper echoes its first argument into column `q`, so a substituted
/// `{abspath}` would surface the absolute path; instead `q` is the literal
/// `{abspath}`.
#[test]
fn on_file_abspath_token_is_not_substituted() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("echo_args.sh"),
        "#!/bin/sh\nprintf '[{\"q\":\"%s\"}]' \"$1\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (q TEXT)"
glob = "*.json"
on-file = "sh echo_args.sh {abspath}"
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

/// The command runs once for the table, with every matched path as a trailing
/// argument: a helper that reports its argument count sees both files in one
/// invocation and lands one row.
#[test]
fn on_file_runs_once_with_every_matched_path_appended() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("count.sh"),
        "#!/bin/sh\nprintf '[{\"n\":%s}]' \"$#\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (n INTEGER)"
glob = "*.json"
on-file = "sh count.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), "x").unwrap();
    fs::write(root.path().join("b.json"), "y").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT n FROM items").unwrap();
    assert_eq!(rows.len(), 1, "one invocation, one row: {rows:?}");
    assert_eq!(rows[0]["n"], Value::Integer(2));
}

/// A `{path}` in the command belongs to the retired per-file contract, so the
/// build refuses it up front and says what to do instead.
#[test]
fn on_file_with_a_path_placeholder_is_rejected_at_build() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
on-file = "cat {path}"
"#,
    )
    .unwrap();

    let err = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .err()
        .expect("`{path}` must be rejected")
        .to_string();
    assert!(err.contains("cat {path}"), "names the command: {err}");
    assert!(
        err.contains("trailing arguments"),
        "names the contract: {err}"
    );
}

/// Every appended path is the matched file's **absolute** path, so an `on-file`
/// script receives self-sufficient arguments that resolve from any cwd. The
/// helper exits non-zero unless its argument is absolute; only then does it
/// `cat` the file. Rows landing proves the script saw an absolute path.
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
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
on-file = "sh abscheck.sh"
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
        "an absolute path must pass the /*-guard and let the script cat the file"
    );
    assert_eq!(rows[0]["name"], Value::Text("widget".into()));
}

/// When the index root differs from the config file's directory (here via an
/// explicit `.root(...)`, since #540 removed the config `root` key), the hook
/// still runs with cwd = the config dir, so a root-relative path would not
/// resolve. The absolute path does: the script `cat`s the file from a cwd
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
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "**/meta.json"
on-file = "sh abscheck.sh"
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

/// The command runs once for the whole table, so its exit status is the
/// table's: a non-zero exit fails the build, naming the table and carrying
/// the command's stderr. The helper (kept out of the TOML to sidestep
/// nested-quote parsing) fails when any file it is handed contains `BOOM`.
#[test]
fn a_failing_command_fails_the_build() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q BOOM \"$@\"; then echo 'boom seen' >&2; exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    let err = build_err(&root);

    let DirSqlError::TableCommand { name, message } = err else {
        panic!("a failed command is the table's error, got: {err}");
    };
    assert_eq!(name, "items");
    assert!(
        message.contains("boom seen"),
        "the failure carries the command's stderr: {message}"
    );
}

fn build_err(root: &TempDir) -> DirSqlError {
    DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .err()
        .expect("a failed command must fail the build")
}

/// Output that is not a JSON array of objects fails the build the same way.
#[test]
fn malformed_output_fails_the_build() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nprintf 'not json'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("good.txt"), "GOOD\n").unwrap();

    let err = build_err(&root);

    assert!(
        matches!(&err, DirSqlError::TableCommand { name, message }
            if name == "items" && message.contains("not a JSON array of rows")),
        "got: {err}"
    );
}

/// Bounding a hook is the command's job now: a `timeout(1)`-wrapped hook that
/// overruns its bound exits non-zero, which fails the build like any other
/// hook failure.
#[test]
fn a_timeout_wrapped_hook_that_overruns_fails_the_build() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("slow.sh"),
        "#!/bin/sh\nsleep 2\nprintf '[{\"name\":\"late\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "timeout 0.5 sh slow.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x\n").unwrap();

    let err = build_err(&root);

    assert!(
        matches!(&err, DirSqlError::TableCommand { name, .. } if name == "items"),
        "got: {err}"
    );
}

/// A slow hook is no longer killed by any built-in bound: a 2-second run
/// (over what used to be configurable) completes and lands its rows.
#[test]
fn a_slow_unwrapped_hook_runs_to_completion() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("slowish.sh"),
        "#!/bin/sh\nsleep 2\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh slowish.sh"
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

/// A row that fails strict normalization is the hook's mistake, and the
/// command ran once for the whole table, so it fails the build under the
/// table's name.
#[test]
fn a_strict_violation_fails_the_build() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("gen.sh"),
        "#!/bin/sh\nprintf '[{\"name\":\"ok\"},{\"nope\":1}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
strict = true
on-file = "sh gen.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "fine\n").unwrap();

    let err = build_err(&root);

    assert!(
        matches!(&err, DirSqlError::TableCommand { name, .. } if name == "items"),
        "got: {err}"
    );
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
            "items",
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
            "items",
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
