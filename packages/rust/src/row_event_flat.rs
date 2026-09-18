//! Flattening a [`RowEvent`] into the six-field shape both bindings expose.
//!
//! Every binding publishes the same `{table, action, row, old_row, error,
//! file_path}` record, and none of the decisions behind it are host-specific:
//! which variant carries an old row, which one may have no table, and the
//! lowercase action names all belong to the core enum. Only the marshaling of
//! the borrowed rows into host values is per-binding.
//!
//! Rows are borrowed so the clone stays at the binding boundary, where it is
//! already paid.

use crate::Row;
#[cfg(test)]
use crate::db::Value;
use crate::differ::RowEvent;

/// A [`RowEvent`] reduced to the flat record the bindings publish.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatRowEvent<'a> {
    pub table: Option<String>,
    pub action: &'static str,
    pub row: Option<&'a Row>,
    pub old_row: Option<&'a Row>,
    pub error: Option<String>,
    pub file_path: String,
}

pub fn flatten_row_event(event: &RowEvent) -> FlatRowEvent<'_> {
    match event {
        RowEvent::Insert {
            table,
            row,
            file_path,
        } => FlatRowEvent {
            table: Some(table.clone()),
            action: "insert",
            row: Some(row),
            old_row: None,
            error: None,
            file_path: file_path.clone(),
        },
        RowEvent::Update {
            table,
            old_row,
            new_row,
            file_path,
        } => FlatRowEvent {
            table: Some(table.clone()),
            action: "update",
            row: Some(new_row),
            old_row: Some(old_row),
            error: None,
            file_path: file_path.clone(),
        },
        RowEvent::Delete {
            table,
            row,
            file_path,
        } => FlatRowEvent {
            table: Some(table.clone()),
            action: "delete",
            row: Some(row),
            old_row: None,
            error: None,
            file_path: file_path.clone(),
        },
        RowEvent::Error {
            table,
            file_path,
            error,
        } => FlatRowEvent {
            table: table.clone(),
            action: "error",
            row: None,
            old_row: None,
            error: Some(error.clone()),
            file_path: file_path.to_string_lossy().to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn one_row(v: i64) -> Row {
        Row::from([("k".to_string(), Value::Integer(v))])
    }

    #[test]
    fn insert_maps_row_and_action() {
        let event = RowEvent::Insert {
            table: "t".into(),
            row: one_row(7),
            file_path: "/f".into(),
        };
        let flat = flatten_row_event(&event);
        assert_eq!(flat.action, "insert");
        assert_eq!(flat.table.as_deref(), Some("t"));
        assert_eq!(flat.row.and_then(|r| r.get("k")), Some(&Value::Integer(7)));
        assert!(flat.old_row.is_none());
        assert!(flat.error.is_none());
        assert_eq!(flat.file_path, "/f");
    }

    #[test]
    fn update_carries_new_row_as_row_and_old_row_beside_it() {
        let event = RowEvent::Update {
            table: "t".into(),
            old_row: one_row(7),
            new_row: one_row(9),
            file_path: "/f".into(),
        };
        let flat = flatten_row_event(&event);
        assert_eq!(flat.action, "update");
        assert_eq!(flat.table.as_deref(), Some("t"));
        assert_eq!(flat.row.and_then(|r| r.get("k")), Some(&Value::Integer(9)));
        assert_eq!(
            flat.old_row.and_then(|r| r.get("k")),
            Some(&Value::Integer(7))
        );
        assert!(flat.error.is_none());
        assert_eq!(flat.file_path, "/f");
    }

    #[test]
    fn delete_has_a_row_but_no_old_row() {
        let event = RowEvent::Delete {
            table: "t".into(),
            row: one_row(7),
            file_path: "/f".into(),
        };
        let flat = flatten_row_event(&event);
        assert_eq!(flat.action, "delete");
        assert_eq!(flat.table.as_deref(), Some("t"));
        assert_eq!(flat.row.and_then(|r| r.get("k")), Some(&Value::Integer(7)));
        assert!(flat.old_row.is_none());
        assert!(flat.error.is_none());
        assert_eq!(flat.file_path, "/f");
    }

    #[test]
    fn error_has_no_rows_and_stringifies_its_path() {
        let event = RowEvent::Error {
            table: None,
            file_path: PathBuf::from("/f"),
            error: "boom".into(),
        };
        let flat = flatten_row_event(&event);
        assert_eq!(flat.action, "error");
        assert!(flat.table.is_none());
        assert!(flat.row.is_none());
        assert!(flat.old_row.is_none());
        assert_eq!(flat.error.as_deref(), Some("boom"));
        assert_eq!(flat.file_path, "/f");
    }

    #[test]
    fn error_keeps_the_table_when_the_variant_names_one() {
        let event = RowEvent::Error {
            table: Some("t".into()),
            file_path: PathBuf::from("/f"),
            error: "boom".into(),
        };
        let flat = flatten_row_event(&event);
        assert_eq!(flat.table.as_deref(), Some("t"));
    }
}
