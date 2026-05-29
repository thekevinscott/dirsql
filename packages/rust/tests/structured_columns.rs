//! Integration / golden tests for structured column definitions (issue #202).
//!
//! Exercises the Rust SDK public API: [`Table::from_columns`], [`Column`],
//! [`ColumnType`], the table-level fields, and [`Table::to_ddl`]. The golden
//! `to_ddl` assertions pin the exact `CREATE TABLE` text so the Python / TS /
//! TOML bindings all have one schema to converge on. The end-to-end cases
//! build a real `DirSQL` and read the schema back through the query API.

use std::collections::HashMap;
use std::fs;

use dirsql::{
    Column, ColumnType, DefaultValue, DirSQL, DirSqlError, Expression, GeneratedColumn,
    GeneratedMode, Index, Row, Table, Value,
};
use tempfile::TempDir;

fn no_rows(_path: &str) -> Vec<Row> {
    Vec::new()
}

#[test]
fn to_ddl_golden_basic() {
    let t = Table::from_columns(
        "docs",
        "**/*.md",
        vec![
            Column::new("title", ColumnType::Text),
            Column::new("body", ColumnType::Text),
        ],
        no_rows,
    );
    assert_eq!(
        t.to_ddl().unwrap(),
        "CREATE TABLE docs (title TEXT, body TEXT, \
         _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL)"
    );
}

#[test]
fn to_ddl_golden_every_constraint_and_escape_hatch() {
    let t = Table::from_columns(
        "t",
        "*.md",
        vec![
            Column {
                name: "id".into(),
                ty: ColumnType::Integer,
                primary_key: true,
                autoincrement: true,
                ..Default::default()
            },
            Column {
                name: "slug".into(),
                ty: ColumnType::Text,
                not_null: true,
                unique: true,
                collate: Some("NOCASE".into()),
                default: Some(DefaultValue::Text("untitled".into())),
                ..Default::default()
            },
            Column {
                name: "ingested_at".into(),
                ty: ColumnType::Integer,
                default: Some(DefaultValue::Sql("strftime('%s', 'now')".into())),
                ..Default::default()
            },
            Column {
                name: "body".into(),
                ty: ColumnType::Text,
                check: Some(Expression {
                    sql: "length(body) > 0".into(),
                }),
                ..Default::default()
            },
            Column {
                name: "body_len".into(),
                ty: ColumnType::Integer,
                generated: Some(GeneratedColumn {
                    sql: "length(body)".into(),
                    mode: GeneratedMode::Stored,
                }),
                ..Default::default()
            },
        ],
        no_rows,
    );
    assert_eq!(
        t.to_ddl().unwrap(),
        "CREATE TABLE t (\
         id INTEGER PRIMARY KEY AUTOINCREMENT, \
         slug TEXT NOT NULL UNIQUE COLLATE NOCASE DEFAULT 'untitled', \
         ingested_at INTEGER DEFAULT (strftime('%s', 'now')), \
         body TEXT CHECK (length(body) > 0), \
         body_len INTEGER GENERATED ALWAYS AS (length(body)) STORED, \
         _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL)"
    );
}

#[test]
fn to_ddl_golden_table_level() {
    let mut t = Table::from_columns(
        "t",
        "*.md",
        vec![
            Column::new("a", ColumnType::Text),
            Column::new("b", ColumnType::Text),
        ],
        no_rows,
    );
    t.primary_key = vec!["a".into(), "b".into()];
    t.unique = vec![vec!["a".into(), "b".into()]];
    t.without_rowid = true;
    assert_eq!(
        t.to_ddl().unwrap(),
        "CREATE TABLE t (a TEXT, b TEXT, \
         _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL, \
         PRIMARY KEY (a, b), UNIQUE (a, b)) WITHOUT ROWID"
    );

    t.indexes = vec![Index {
        name: Some("idx_b".into()),
        columns: vec!["b".into()],
        unique: false,
    }];
    assert_eq!(t.index_ddls().unwrap(), vec!["CREATE INDEX idx_b ON t (b)"]);
}

#[test]
fn legacy_ddl_table_still_builds() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "x").unwrap();
    let t = Table::new("CREATE TABLE docs (title TEXT)", "*.md", |_| {
        vec![HashMap::from([("title".into(), Value::Text("hi".into()))])]
    });
    assert!(!t.is_columns_based());
    let db = DirSQL::new(root.path(), vec![t]).unwrap();
    let rows = db.query("SELECT title FROM docs").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], Value::Text("hi".into()));
}

#[test]
fn structured_table_indexes_data_end_to_end() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "x").unwrap();
    let mut t = Table::from_columns(
        "docs",
        "*.md",
        vec![
            Column {
                name: "title".into(),
                ty: ColumnType::Text,
                not_null: true,
                ..Default::default()
            },
            Column::new("body", ColumnType::Text),
        ],
        |_| {
            vec![HashMap::from([
                ("title".into(), Value::Text("hi".into())),
                ("body".into(), Value::Text("world".into())),
            ])]
        },
    );
    t.indexes = vec![Index {
        name: Some("idx_title".into()),
        columns: vec!["title".into()],
        unique: true,
    }];

    let db = DirSQL::new(root.path(), vec![t]).unwrap();

    let rows = db.query("SELECT title, body FROM docs").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], Value::Text("hi".into()));

    // Schema round-trips: the NOT NULL constraint landed.
    let info = db
        .query("SELECT name, \"notnull\" AS nn FROM pragma_table_info('docs') WHERE name = 'title'")
        .unwrap();
    assert_eq!(info[0]["nn"], Value::Integer(1));

    // The declared index exists and is unique.
    let idx = db
        .query(
            "SELECT name, \"unique\" AS uq FROM pragma_index_list('docs') WHERE name = 'idx_title'",
        )
        .unwrap();
    assert_eq!(idx.len(), 1);
    assert_eq!(idx[0]["uq"], Value::Integer(1));
}

#[test]
fn composite_primary_key_builds_and_queries() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "x").unwrap();
    let mut t = Table::from_columns(
        "t",
        "*.md",
        vec![
            Column::new("a", ColumnType::Text),
            Column::new("b", ColumnType::Text),
        ],
        |_| {
            vec![HashMap::from([
                ("a".into(), Value::Text("1".into())),
                ("b".into(), Value::Text("2".into())),
            ])]
        },
    );
    t.primary_key = vec!["a".into(), "b".into()];
    t.unique = vec![vec!["a".into(), "b".into()]];

    // SQLite executes the rendered DDL here, so an invalid column/constraint
    // order would surface as a build error.
    let db = DirSQL::new(root.path(), vec![t]).unwrap();

    let rows = db.query("SELECT a, b FROM t").unwrap();
    assert_eq!(rows.len(), 1);

    let pk = db
        .query("SELECT name, pk FROM pragma_table_info('t') WHERE pk > 0 ORDER BY pk")
        .unwrap();
    assert_eq!(pk.len(), 2);
    assert_eq!(pk[0]["name"], Value::Text("a".into()));
    assert_eq!(pk[1]["name"], Value::Text("b".into()));
}

#[test]
fn mixing_ddl_and_columns_is_rejected() {
    let mut t = Table::from_columns(
        "t",
        "*.md",
        vec![Column::new("x", ColumnType::Text)],
        no_rows,
    );
    t.ddl = Some("CREATE TABLE t (x TEXT)".into());

    let err = t.resolved_name().unwrap_err();
    assert!(matches!(err, DirSqlError::MixedTableDefinition(_)));

    // And it surfaces when building a DirSQL.
    let root = TempDir::new().unwrap();
    let build = DirSQL::new(root.path(), vec![t]);
    assert!(matches!(build, Err(DirSqlError::MixedTableDefinition(_))));
}

#[test]
fn structured_table_without_a_name_errors() {
    let t = Table::from_columns(
        "",
        "*.md",
        vec![Column::new("x", ColumnType::Text)],
        no_rows,
    );
    assert!(matches!(t.resolved_name(), Err(DirSqlError::Schema(_))));
    assert!(matches!(t.to_ddl(), Err(DirSqlError::Schema(_))));
}

#[test]
fn to_ddl_on_legacy_table_errors() {
    let t = Table::new("CREATE TABLE t (x TEXT)", "*.md", no_rows);
    assert!(matches!(t.to_ddl(), Err(DirSqlError::Schema(_))));
}

#[test]
fn legacy_unparseable_ddl_errors() {
    let t = Table::new("NOT A CREATE TABLE", "*.md", no_rows);
    assert!(matches!(t.resolved_name(), Err(DirSqlError::Ddl(_))));
}

#[test]
fn strict_types_renders_strict_table() {
    let mut t = Table::from_columns(
        "t",
        "*.md",
        vec![Column::new("title", ColumnType::Text)],
        no_rows,
    );
    t.strict_types = true;
    assert!(t.to_ddl().unwrap().ends_with(") STRICT"));
}
