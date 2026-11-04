//! Integration tests for bind parameters in the core `query` path: the values
//! reach SQLite's binding, for path-tables and declared tables alike, and a
//! mismatch between the placeholders and the values is an error naming it.

use std::collections::HashMap;
use std::fs;

use dirsql::{AsyncDirSQL, DirSQL, Params, Row, Table, Value};
use tempfile::TempDir;

const TRICKY: &str = "it's \"quoted\"'; DROP TABLE notes; -- ünïcødé ✓";

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    let advice = serde_json::json!({ "advice": TRICKY }).to_string();
    fs::write(root.path().join("a.json"), advice).unwrap();
    fs::write(root.path().join("b.json"), r#"{"advice": "plain"}"#).unwrap();
    root
}

fn notes_table() -> Table {
    Table::new(
        "notes",
        "CREATE TABLE notes (name TEXT, body TEXT)",
        "*.json",
        |path| {
            let name = std::path::Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let parsed: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            vec![HashMap::from([
                ("name".into(), Value::Text(name)),
                (
                    "body".into(),
                    Value::Text(parsed["advice"].as_str().unwrap().to_string()),
                ),
            ])]
        },
    )
}

fn named(pairs: &[(&str, Value)]) -> Params {
    Params::Named(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect(),
    )
}

fn paths(rows: &[Row]) -> Vec<Value> {
    rows.iter().map(|row| row["path"].clone()).collect()
}

#[test]
fn a_named_parameter_filters_a_path_table() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    let rows = db
        .query_with(
            "SELECT path FROM './*.json' WHERE content ->> 'advice' = :advice",
            &named(&[("advice", Value::Text(TRICKY.into()))]),
        )
        .unwrap();

    assert_eq!(paths(&rows), vec![Value::Text("a.json".into())]);
}

#[test]
fn a_positional_parameter_filters_a_declared_table() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![notes_table()]).unwrap();

    let rows = db
        .query_with(
            "SELECT name, body FROM notes WHERE body = ?",
            &Params::Positional(vec![Value::Text(TRICKY.into())]),
        )
        .unwrap();

    assert_eq!(
        rows,
        vec![HashMap::from([
            ("name".into(), Value::Text("a.json".into())),
            ("body".into(), Value::Text(TRICKY.into())),
        ])]
    );
}

#[test]
fn each_native_type_binds_as_itself() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    let result = db
        .query_ordered_with(
            "SELECT typeof(?1) AS t1, ?1 AS v1, typeof(?2) AS t2, ?2 AS v2, \
             typeof(?3) AS t3, ?3 AS v3, typeof(?4) AS t4, ?4 AS v4, typeof(?5) AS t5",
            &Params::Positional(vec![
                Value::Integer(7),
                Value::Real(1.5),
                Value::Text("x".into()),
                Value::Blob(vec![0, 1, 2]),
                Value::Null,
            ]),
        )
        .unwrap();

    assert_eq!(
        result.columns,
        vec!["t1", "v1", "t2", "v2", "t3", "v3", "t4", "v4", "t5"]
    );
    let row = &result.rows[0];
    assert_eq!(row["t1"], Value::Text("integer".into()));
    assert_eq!(row["v1"], Value::Integer(7));
    assert_eq!(row["t2"], Value::Text("real".into()));
    assert_eq!(row["v2"], Value::Real(1.5));
    assert_eq!(row["t3"], Value::Text("text".into()));
    assert_eq!(row["v3"], Value::Text("x".into()));
    assert_eq!(row["t4"], Value::Text("blob".into()));
    assert_eq!(row["v4"], Value::Blob(vec![0, 1, 2]));
    assert_eq!(row["t5"], Value::Text("null".into()));
}

#[test]
fn query_each_binds_for_a_path_table() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    let seen = db
        .query_each_with(
            "SELECT path, $tag AS tag FROM './*.json' WHERE content ->> 'advice' = @advice",
            &named(&[
                ("advice", Value::Text("plain".into())),
                ("tag", Value::Integer(3)),
            ]),
            |columns| (columns.to_vec(), Vec::new()),
            |(_, rows), cells| rows.push(cells.to_vec()),
        )
        .unwrap();

    assert_eq!(seen.0, vec!["path", "tag"]);
    assert_eq!(
        seen.1,
        vec![vec![Value::Text("b.json".into()), Value::Integer(3)]]
    );
}

#[test]
fn query_each_binds_for_a_declared_table() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![notes_table()]).unwrap();

    let rows = db
        .query_each_with(
            "SELECT name FROM notes WHERE body = ?",
            &Params::Positional(vec![Value::Text("plain".into())]),
            |_| Vec::new(),
            |rows, cells| rows.push(cells.to_vec()),
        )
        .unwrap();

    assert_eq!(rows, vec![vec![Value::Text("b.json".into())]]);
}

#[test]
fn a_wrong_parameter_count_is_an_error() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    let err = db
        .query_with(
            "SELECT ? AS a, ? AS b",
            &Params::Positional(vec![Value::Integer(1)]),
        )
        .unwrap_err();

    assert_eq!(
        err.to_string(),
        "the query has 2 parameter(s), but 1 value(s) were given"
    );
}

#[test]
fn a_missing_named_parameter_is_an_error() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    let err = db
        .query_with(
            "SELECT path FROM './*.json' WHERE content ->> 'advice' = :advice",
            &named(&[]),
        )
        .unwrap_err();

    assert_eq!(err.to_string(), "the named parameter :advice has no value");
}

#[test]
fn a_mapping_key_with_no_placeholder_is_an_error() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![notes_table()]).unwrap();

    let err = db
        .query_with(
            "SELECT name FROM notes WHERE body = :body",
            &named(&[("body", Value::Null), ("bdoy", Value::Null)]),
        )
        .unwrap_err();

    assert_eq!(
        err.to_string(),
        "the parameter \"bdoy\" is not a placeholder in the query"
    );
}

#[tokio::test]
async fn async_query_binds_parameters() {
    let root = fixture();
    let db = AsyncDirSQL::new(root.path(), vec![notes_table()]).unwrap();
    db.ready().await.unwrap();

    let rows = db
        .query_with(
            "SELECT name FROM notes WHERE body = :body",
            &named(&[("body", Value::Text(TRICKY.into()))]),
        )
        .await
        .unwrap();

    assert_eq!(
        rows,
        vec![HashMap::from([(
            "name".into(),
            Value::Text("a.json".into())
        )])]
    );
}
