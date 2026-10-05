//! `Row` / `RowEvent` → JSON serialization for the HTTP API.

use serde_json::{Map, Value};

use crate::db::Value as CellValue;
use crate::differ::RowEvent;
use crate::row_event_flat::flatten_row_event;
use crate::{QueryResult, Row};

/// Serialize a query result, each object keyed in projection order. Row events
/// carry no projection, so they keep going through [`row_to_json`].
pub(super) fn result_to_json(result: &QueryResult) -> Vec<Value> {
    result
        .rows
        .iter()
        .map(|row| ordered_row_to_json(&result.columns, row))
        .collect()
}

fn ordered_row_to_json(columns: &[String], row: &Row) -> Value {
    let mut map = Map::with_capacity(columns.len());
    for column in columns {
        if let Some(value) = row.get(column) {
            map.insert(column.clone(), cell_to_json(value));
        }
    }
    Value::Object(map)
}

/// A query's rows written one at a time as the JSON array [`result_to_json`]
/// renders, with no map per row.
pub(super) struct JsonRows {
    out: Vec<u8>,
    /// Each key as JSON with the position of the cell it takes.
    keys: Vec<(Vec<u8>, usize)>,
    rows: usize,
}

impl JsonRows {
    pub(super) fn new(columns: &[String]) -> Self {
        let keys = columns
            .iter()
            .enumerate()
            .map(|(i, name)| (json_bytes(name), i))
            .collect();
        Self {
            out: b"[".to_vec(),
            keys,
            rows: 0,
        }
    }

    pub(super) fn row(&mut self, cells: &[CellValue]) {
        if self.rows > 0 {
            self.out.push(b',');
        }
        self.rows += 1;
        self.out.push(b'{');
        for (n, (key, i)) in self.keys.iter().enumerate() {
            if n > 0 {
                self.out.push(b',');
            }
            self.out.extend_from_slice(key);
            self.out.push(b':');
            write_cell(&mut self.out, &cells[*i]);
        }
        self.out.push(b'}');
    }

    /// The array with the newline the CLI prints after it.
    pub(super) fn finish(mut self) -> Vec<u8> {
        self.out.extend_from_slice(b"]\n");
        self.out
    }
}

fn json_bytes(text: &str) -> Vec<u8> {
    serde_json::to_vec(text).expect("a string always serializes")
}

fn write_cell(out: &mut Vec<u8>, value: &CellValue) {
    let written = match value {
        CellValue::Text(text) => serde_json::to_writer(&mut *out, text),
        other => serde_json::to_writer(&mut *out, &cell_to_json(other)),
    };
    written.expect("writing JSON to memory cannot fail");
}

pub(super) fn row_to_json(row: &Row) -> Value {
    let mut map = Map::with_capacity(row.len());
    for (k, v) in row {
        map.insert(k.clone(), cell_to_json(v));
    }
    Value::Object(map)
}

fn cell_to_json(value: &CellValue) -> Value {
    match value {
        CellValue::Null => Value::Null,
        CellValue::Integer(i) => Value::from(*i),
        CellValue::Real(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        CellValue::Text(s) => Value::String(s.clone()),
        CellValue::Blob(bytes) => Value::String(hex::encode(bytes)),
    }
}

pub(super) fn event_to_json(event: &RowEvent) -> String {
    event_to_value(event).to_string()
}

fn event_to_value(event: &RowEvent) -> Value {
    let flat = flatten_row_event(event);
    let mut map = Map::new();
    map.insert("action".into(), Value::from(flat.action));
    map.insert(
        "table".into(),
        flat.table.map_or(Value::Null, Value::String),
    );
    map.insert("file_path".into(), Value::String(flat.file_path));
    match flat.error {
        Some(error) => {
            map.insert("error".into(), Value::String(error));
        }
        None => {
            map.insert("row".into(), flat.row.map_or(Value::Null, row_to_json));
            map.insert(
                "old_row".into(),
                flat.old_row.map_or(Value::Null, row_to_json),
            );
        }
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn row_serializes_to_json_object() {
        let mut row: Row = HashMap::new();
        row.insert("title".into(), CellValue::Text("Hello".into()));
        row.insert("count".into(), CellValue::Integer(3));
        let json = row_to_json(&row);
        assert_eq!(json.get("title").and_then(Value::as_str), Some("Hello"));
        assert_eq!(json.get("count").and_then(Value::as_i64), Some(3));
    }

    #[test]
    fn a_result_serializes_its_rows_in_projection_order() {
        let mut row: Row = HashMap::new();
        row.insert("title".into(), CellValue::Text("Hello".into()));
        row.insert("count".into(), CellValue::Integer(3));
        let result = QueryResult {
            columns: vec!["count".into(), "title".into()],
            rows: vec![row],
        };

        let json = result_to_json(&result);

        assert_eq!(json[0].to_string(), r#"{"count":3,"title":"Hello"}"#);
    }

    #[test]
    fn a_column_no_row_key_matches_is_left_out() {
        let mut row: Row = HashMap::new();
        row.insert("title".into(), CellValue::Text("Hello".into()));
        let result = QueryResult {
            columns: vec!["title".into(), "title".into()],
            rows: vec![row],
        };

        let json = result_to_json(&result);

        assert_eq!(json[0].to_string(), r#"{"title":"Hello"}"#);
    }

    fn rendered(result: &QueryResult) -> String {
        format!("{}\n", Value::Array(result_to_json(result)))
    }

    fn streamed(result: &QueryResult) -> String {
        let mut rows = JsonRows::new(&result.columns);
        for row in &result.rows {
            let cells: Vec<CellValue> = result
                .columns
                .iter()
                .map(|column| row[column].clone())
                .collect();
            rows.row(&cells);
        }
        String::from_utf8(rows.finish()).unwrap()
    }

    fn row_of(cells: &[(&str, CellValue)]) -> Row {
        cells
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn streamed_rows_match_the_rendered_array_cell_for_cell() {
        let result = QueryResult {
            columns: vec!["path".into(), "size".into(), "ratio".into(), "body".into()],
            rows: vec![
                row_of(&[
                    ("path", CellValue::Text("a \"quoted\" \u{e9}\n.md".into())),
                    ("size", CellValue::Integer(-3)),
                    ("ratio", CellValue::Real(0.1)),
                    ("body", CellValue::Blob(vec![0, 255])),
                ]),
                row_of(&[
                    ("path", CellValue::Null),
                    ("size", CellValue::Integer(i64::MAX)),
                    ("ratio", CellValue::Real(f64::NAN)),
                    ("body", CellValue::Text(String::new())),
                ]),
            ],
        };
        assert_eq!(streamed(&result), rendered(&result));
    }

    #[test]
    fn streamed_rows_keep_one_key_per_repeated_column_name() {
        let result = QueryResult {
            columns: vec!["a".into(), "b".into(), "a".into()],
            rows: vec![row_of(&[
                ("a", CellValue::Integer(1)),
                ("b", CellValue::Integer(2)),
            ])],
        };
        assert_eq!(streamed(&result), rendered(&result));
    }

    #[test]
    fn no_rows_stream_as_an_empty_array() {
        let result = QueryResult {
            columns: vec!["path".into()],
            rows: Vec::new(),
        };
        assert_eq!(streamed(&result), "[]\n");
    }

    #[test]
    fn insert_event_emits_expected_shape() {
        let mut row: Row = HashMap::new();
        row.insert("id".into(), CellValue::Text("abc".into()));
        let event = RowEvent::Insert {
            table: "posts".into(),
            row,
            file_path: "posts/a.json".into(),
        };
        let parsed = event_to_value(&event);
        assert_eq!(parsed.get("action").and_then(Value::as_str), Some("insert"));
        assert_eq!(parsed.get("table").and_then(Value::as_str), Some("posts"));
        assert_eq!(
            parsed.get("file_path").and_then(Value::as_str),
            Some("posts/a.json"),
        );
        assert!(parsed.get("old_row").unwrap().is_null());
        assert!(parsed.get("error").is_none());
        assert_eq!(event_to_json(&event), parsed.to_string());
    }

    #[test]
    fn update_event_carries_both_rows() {
        let mut old: Row = HashMap::new();
        old.insert("id".into(), CellValue::Text("abc".into()));
        let mut new: Row = HashMap::new();
        new.insert("id".into(), CellValue::Text("abc2".into()));
        let event = RowEvent::Update {
            table: "posts".into(),
            old_row: old,
            new_row: new,
            file_path: "posts/a.json".into(),
        };
        let parsed = event_to_value(&event);
        assert_eq!(
            parsed.pointer("/row/id").and_then(Value::as_str),
            Some("abc2")
        );
        assert_eq!(
            parsed.pointer("/old_row/id").and_then(Value::as_str),
            Some("abc")
        );
    }

    #[test]
    fn error_event_has_error_field() {
        let event = RowEvent::Error {
            table: Some("posts".into()),
            file_path: PathBuf::from("bad.json"),
            error: "parse failed".into(),
        };
        let parsed = event_to_value(&event);
        assert_eq!(parsed.get("action").and_then(Value::as_str), Some("error"));
        assert_eq!(
            parsed.get("error").and_then(Value::as_str),
            Some("parse failed")
        );
    }

    #[test]
    fn error_event_omits_the_row_keys() {
        let event = RowEvent::Error {
            table: None,
            file_path: PathBuf::from("bad.json"),
            error: "parse failed".into(),
        };
        let parsed = event_to_value(&event);
        assert!(parsed.get("table").unwrap().is_null());
        assert!(parsed.get("row").is_none());
        assert!(parsed.get("old_row").is_none());
    }

    #[test]
    fn delete_event_emits_expected_shape() {
        let mut row: Row = HashMap::new();
        row.insert("id".into(), CellValue::Text("gone".into()));
        let event = RowEvent::Delete {
            table: "posts".into(),
            row,
            file_path: "posts/a.json".into(),
        };
        let parsed = event_to_value(&event);
        assert_eq!(parsed.get("action").and_then(Value::as_str), Some("delete"));
        assert_eq!(parsed.get("table").and_then(Value::as_str), Some("posts"));
        assert_eq!(
            parsed.get("file_path").and_then(Value::as_str),
            Some("posts/a.json"),
        );
        assert_eq!(
            parsed.pointer("/row/id").and_then(Value::as_str),
            Some("gone")
        );
        assert!(parsed.get("old_row").unwrap().is_null());
    }

    #[test]
    fn null_cell_becomes_json_null() {
        assert!(cell_to_json(&CellValue::Null).is_null());
    }

    #[test]
    fn blob_cell_becomes_hex_string() {
        let json = cell_to_json(&CellValue::Blob(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(json.as_str(), Some("deadbeef"));
        let padded = cell_to_json(&CellValue::Blob(vec![0x00, 0x0f, 0xff]));
        assert_eq!(padded.as_str(), Some("000fff"));
    }

    #[test]
    fn non_finite_real_becomes_json_null() {
        assert!(cell_to_json(&CellValue::Real(f64::NAN)).is_null());
        assert!(cell_to_json(&CellValue::Real(f64::INFINITY)).is_null());
    }
}
