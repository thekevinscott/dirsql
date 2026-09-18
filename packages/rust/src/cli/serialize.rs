//! `Row` / `RowEvent` → JSON serialization for the HTTP API.

use serde_json::{Map, Value};

use crate::Row;
use crate::db::Value as CellValue;
use crate::differ::RowEvent;
use crate::row_event_flat::flatten_row_event;

pub(super) fn rows_to_json(rows: &[Row]) -> Vec<Value> {
    rows.iter().map(row_to_json).collect()
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
