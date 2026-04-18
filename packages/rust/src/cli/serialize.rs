//! `Row` / `RowEvent` → JSON serialization for the HTTP API.

use serde_json::{Map, Value, json};

use crate::Row;
use crate::db::Value as CellValue;
use crate::differ::RowEvent;

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
        CellValue::Blob(bytes) => Value::String(hex_encode(bytes)),
    }
}

/// Hex-encode a byte slice. Used for `BLOB` SQLite values in JSON
/// output. Lightweight; pulls in no dep just to emit hex.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub(super) fn event_to_json(event: &RowEvent) -> String {
    let value = match event {
        RowEvent::Insert {
            table,
            row,
            file_path,
        } => json!({
            "action": "insert",
            "table": table,
            "file_path": file_path,
            "row": row_to_json(row),
            "old_row": Value::Null,
        }),
        RowEvent::Update {
            table,
            old_row,
            new_row,
            file_path,
        } => json!({
            "action": "update",
            "table": table,
            "file_path": file_path,
            "row": row_to_json(new_row),
            "old_row": row_to_json(old_row),
        }),
        RowEvent::Delete {
            table,
            row,
            file_path,
        } => json!({
            "action": "delete",
            "table": table,
            "file_path": file_path,
            "row": row_to_json(row),
            "old_row": Value::Null,
        }),
        RowEvent::Error {
            table,
            file_path,
            error,
        } => json!({
            "action": "error",
            "table": table,
            "file_path": file_path.to_string_lossy(),
            "error": error,
        }),
    };
    value.to_string()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

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
        let s = event_to_json(&event);
        let parsed: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.get("action").and_then(Value::as_str), Some("insert"));
        assert_eq!(parsed.get("table").and_then(Value::as_str), Some("posts"));
        assert_eq!(
            parsed.get("file_path").and_then(Value::as_str),
            Some("posts/a.json"),
        );
        assert!(parsed.get("old_row").unwrap().is_null());
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
        let parsed: Value = serde_json::from_str(&event_to_json(&event)).unwrap();
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
        let parsed: Value = serde_json::from_str(&event_to_json(&event)).unwrap();
        assert_eq!(parsed.get("action").and_then(Value::as_str), Some("error"));
        assert_eq!(
            parsed.get("error").and_then(Value::as_str),
            Some("parse failed")
        );
    }

    #[test]
    fn null_cell_becomes_json_null() {
        assert!(cell_to_json(&CellValue::Null).is_null());
    }

    #[test]
    fn non_finite_real_becomes_json_null() {
        assert!(cell_to_json(&CellValue::Real(f64::NAN)).is_null());
        assert!(cell_to_json(&CellValue::Real(f64::INFINITY)).is_null());
    }

    #[test]
    fn hex_encode_matches_lowercase_spec() {
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xff, 0xde, 0xad]), "000fffdead");
    }
}
