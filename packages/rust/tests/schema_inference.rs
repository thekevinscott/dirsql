//! Integration tests for schema inference from row-object output: a real
//! parser command over a real temp tree, a real SQLite connection, and the
//! schema the parsed path-table declares from what the parser emitted.
//!
//! The pure inference function is unit-tested inline in `src/infer.rs`; this
//! file proves the end-to-end shape — parser runs, schema is declared, rows
//! are queryable.

use std::fs;

use dirsql::parsed_vtab::load_module;
use rusqlite::Connection;
use tempfile::TempDir;

/// A parser command that echoes each file's body verbatim. The on-file
/// contract's payload is the last non-empty line of stdout, so a file whose
/// content is one line of JSON is its own parser output.
const CAT_PARSER: &str = "cat {path}";

/// A connection with the parsed path-table module registered and one vtab
/// named `t` over `glob` under `dir`, parsed by `command`.
fn open_over(dir: &TempDir, glob: &str, command: &str) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn).unwrap();
    // The trailing empty argument is the cache path: this vtab is ephemeral,
    // so there is nowhere to reuse rows from.
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE t USING dirsql_parsed('{}', '{}', '{}', 'gitignore', '')",
        dir.path().display(),
        glob,
        command
    ))
    .unwrap();
    conn
}

fn write(dir: &TempDir, name: &str, body: &str) {
    fs::write(dir.path().join(name), body).unwrap();
}

fn column_names(conn: &Connection, sql: &str) -> Vec<String> {
    let stmt = conn.prepare(sql).unwrap();
    stmt.column_names().into_iter().map(String::from).collect()
}

fn declared_types(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT name, type FROM pragma_table_info('t')")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn columns_are_inferred_from_row_objects_and_rows_are_queryable() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"title":"one","n":1}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    assert_eq!(column_names(&conn, "SELECT * FROM t"), vec!["title", "n"]);

    let (title, n): (String, i64) = conn
        .query_row("SELECT title, n FROM t", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(title, "one");
    assert_eq!(n, 1);
}

#[test]
fn columns_are_the_union_of_keys_across_every_row() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"a":1},{"b":2}]"#);
    write(&dir, "b.json", r#"[{"c":3}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let mut cols = column_names(&conn, "SELECT * FROM t");
    cols.sort();
    assert_eq!(cols, vec!["a", "b", "c"]);
}

#[test]
fn column_order_is_first_seen_so_select_star_is_stable() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"zeta":1,"alpha":2},{"middle":3}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    assert_eq!(
        column_names(&conn, "SELECT * FROM t"),
        vec!["zeta", "alpha", "middle"],
        "columns keep the order the parser first emitted them, not sorted order"
    );
}

#[test]
fn json_types_map_to_sqlite_types() {
    let dir = TempDir::new().unwrap();
    write(
        &dir,
        "a.json",
        r#"[{"s":"x","i":1,"f":1.5,"b":true,"nested":{"k":"v"}}]"#,
    );
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    assert_eq!(
        declared_types(&conn),
        vec![
            ("s".to_string(), "TEXT".to_string()),
            ("i".to_string(), "INTEGER".to_string()),
            ("f".to_string(), "REAL".to_string()),
            ("b".to_string(), "INTEGER".to_string()),
            ("nested".to_string(), "TEXT".to_string()),
        ]
    );
}

#[test]
fn a_key_that_is_never_non_null_is_text() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"maybe":null},{"maybe":null}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    assert_eq!(
        declared_types(&conn),
        vec![("maybe".to_string(), "TEXT".to_string())]
    );
}

#[test]
fn a_key_null_in_one_row_takes_its_type_from_another() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"n":null},{"n":7}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    assert_eq!(
        declared_types(&conn),
        vec![("n".to_string(), "INTEGER".to_string())]
    );

    let values: Vec<Option<i64>> = {
        let mut stmt = conn.prepare("SELECT n FROM t").unwrap();
        let v = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        v
    };
    assert_eq!(values, vec![None, Some(7)]);
}

#[test]
fn a_key_missing_from_one_row_is_null_there() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"a":1,"b":"x"},{"a":2}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let bs: Vec<Option<String>> = {
        let mut stmt = conn.prepare("SELECT b FROM t").unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(bs, vec![Some("x".to_string()), None]);
}

#[test]
fn conflicting_types_across_rows_fall_back_to_text() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"mixed":1},{"mixed":"two"}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    assert_eq!(
        declared_types(&conn),
        vec![("mixed".to_string(), "TEXT".to_string())]
    );
}

#[test]
fn nested_objects_and_arrays_are_stored_as_json_text() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"obj":{"k":"v"},"arr":[1,2]}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let (obj, arr): (String, String) = conn
        .query_row("SELECT obj, arr FROM t", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(obj, r#"{"k":"v"}"#);
    assert_eq!(arr, "[1,2]");
}

#[test]
fn rows_from_every_matched_file_are_present() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"id":"a1"},{"id":"a2"}]"#);
    write(&dir, "b.json", r#"[{"id":"b1"}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let mut stmt = conn.prepare("SELECT id FROM t ORDER BY id").unwrap();
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, vec!["a1", "a2", "b1"]);
}

#[test]
fn the_glob_scopes_which_files_the_parser_sees() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"id":"kept"}]"#);
    write(&dir, "b.txt", r#"[{"id":"skipped"}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let mut stmt = conn.prepare("SELECT id FROM t").unwrap();
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, vec!["kept"]);
}

#[test]
fn a_parser_producing_no_rows_is_an_error_at_registration() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", "[]");
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn).unwrap();

    let err = conn
        .execute_batch(&format!(
            "CREATE VIRTUAL TABLE t USING dirsql_parsed('{}', '**/*.json', '{CAT_PARSER}', \
             'gitignore', '')",
            dir.path().display()
        ))
        .unwrap_err();

    assert!(
        err.to_string().contains("no rows"),
        "an empty sample cannot yield a schema; got: {err}"
    );
}

#[test]
fn a_failing_file_is_skipped_and_the_good_files_survive() {
    // Per-file isolation (the `on-file` hook contract #631 finalizes): a file
    // whose parser output does not parse contributes no rows, and the scan
    // continues. Registration succeeds; the good file's rows are queryable and
    // the schema is inferred from what did parse.
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"id":"kept"}]"#);
    write(&dir, "bad.json", "not valid json");
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let mut stmt = conn.prepare("SELECT id FROM t").unwrap();
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, vec!["kept"]);
}

#[test]
fn a_scan_where_every_file_fails_cannot_infer_a_schema() {
    // With no file parsing, there is no sample to infer from — the same
    // no-rows error an empty sample raises.
    let dir = TempDir::new().unwrap();
    write(&dir, "bad.json", "not valid json");
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn).unwrap();

    let err = conn
        .execute_batch(&format!(
            "CREATE VIRTUAL TABLE t USING dirsql_parsed('{}', '**/*.json', '{CAT_PARSER}', \
             'gitignore', '')",
            dir.path().display()
        ))
        .unwrap_err();

    assert!(
        err.to_string().contains("no rows"),
        "an all-skipped scan yields no schema; got: {err}"
    );
}

#[test]
fn writes_are_rejected() {
    let dir = TempDir::new().unwrap();
    write(&dir, "a.json", r#"[{"id":"a"}]"#);
    let conn = open_over(&dir, "**/*.json", CAT_PARSER);

    let err = conn.execute("DELETE FROM t", []).unwrap_err();
    assert!(
        err.to_string().contains("may not be modified"),
        "read-only is enforced by omitting xUpdate; got: {err}"
    );
}
