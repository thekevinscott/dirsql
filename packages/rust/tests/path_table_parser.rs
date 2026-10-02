//! Integration tests for `DirSQLBuilder::path_table_parser` — the SDK hook the
//! CLI's `--on-file` flag drives. Real filesystem, real SQLite, real parser
//! process, SDK public API. With a parser attached, a path-table's rows and
//! schema come from the command's JSON output instead of the stat columns.

use std::fs;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

/// A parser that hands back the files' bodies combined into one array: each
/// file is a one-line JSON array of row objects, so stripping its brackets and
/// joining the bodies with commas is the table's output.
const COMBINE_SCRIPT: &str = "#!/bin/sh\nprintf '['\nsep=''\nfor f; do\n  body=$(cat \"$f\")\n  body=${body#'['}\n  body=${body%']'}\n  if [ -n \"$body\" ]; then printf '%s%s' \"$sep\" \"$body\"; sep=','; fi\ndone\nprintf ']'\n";

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("combine.sh"), COMBINE_SCRIPT).unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(
        root.path().join("docs/a.md"),
        r#"[{"title":"alpha","n":1}]"#,
    )
    .unwrap();
    fs::write(
        root.path().join("docs/b.md"),
        r#"[{"title":"bravo","n":2}]"#,
    )
    .unwrap();
    root
}

fn open_with_parser(root: &TempDir) -> DirSQL {
    let parser = format!(
        "sh {}",
        shlex::try_quote(&root.path().join("combine.sh").to_string_lossy()).unwrap()
    );
    DirSQL::builder()
        .root(root.path())
        .path_table_parser(&parser)
        .build()
        .unwrap()
}

fn texts(rows: &[Row], column: &str) -> Vec<String> {
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| match r.get(column) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("{column} was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_parsed_path_table_serves_the_parser_rows_and_schema() {
    let root = fixture();
    let db = open_with_parser(&root);

    let rows = db.query("SELECT title, n FROM './docs/*.md'").unwrap();

    assert_eq!(texts(&rows, "title"), vec!["alpha", "bravo"]);
    // `n` is a parser column; a stat path-table never had it.
    assert!(
        rows.iter()
            .all(|r| matches!(r.get("n"), Some(Value::Integer(_)))),
        "the parser's `n` column is present and integer-typed: {rows:?}"
    );
}

#[test]
fn stat_columns_are_not_reachable_on_a_parsed_path_table() {
    let root = fixture();
    let db = open_with_parser(&root);

    // `size` is a stat column; a parsed table's schema is the parser's output
    // alone, so selecting it is a plain missing-column error.
    let err = db
        .query("SELECT size FROM './docs/*.md'")
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("no such column") || err.contains("size"),
        "a stat column must not resolve on a parsed table; got: {err}"
    );
}

#[test]
fn a_parsed_scan_honors_the_default_ignore_rules() {
    let root = fixture();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::write(
        root.path().join("node_modules/pkg/dep.md"),
        r#"[{"title":"dependency","n":9}]"#,
    )
    .unwrap();

    let db = open_with_parser(&root);
    let rows = db.query("SELECT title FROM './**/*.md'").unwrap();

    assert_eq!(
        texts(&rows, "title"),
        vec!["alpha", "bravo"],
        "node_modules must be skipped by a parsed scan too"
    );
}

#[test]
fn a_file_the_parser_cannot_handle_fails_the_whole_table() {
    let root = fixture();
    fs::write(root.path().join("docs/bad.md"), "not valid json").unwrap();

    let db = open_with_parser(&root);
    let err = db
        .query("SELECT title FROM './docs/*.md'")
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("not a JSON array of rows"),
        "the parser's output is the table's output, so one bad file fails it: {err}"
    );
}

#[cfg(unix)]
#[test]
fn a_parser_is_spawned_in_the_index_root_and_root_names_it() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture();
    let script = root.path().join("parse.sh");
    fs::write(&script, "#!/bin/sh\nprintf '[{\"root\":\"%s\"}]' \"$1\"\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let db = open_with_parser(&root, "./parse.sh {root}");

    let rows = db.query("SELECT root FROM './docs/*.md'").unwrap();

    let index_root = root.path().to_string_lossy().into_owned();
    assert_eq!(
        texts(&rows, "root"),
        vec![index_root.clone(), index_root],
        "a relative parser resolves against the index root, which {{root}} names"
    );
}
