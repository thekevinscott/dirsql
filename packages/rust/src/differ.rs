use std::collections::HashMap;
use std::path::PathBuf;

use crate::db::Value;

/// Events produced by comparing old and new file content.
///
/// All variants include the source file's relative path (`file_path`) so
/// downstream consumers can attribute row events to their originating file.
#[derive(Debug, Clone, PartialEq)]
pub enum RowEvent {
    Insert {
        table: String,
        row: HashMap<String, Value>,
        file_path: String,
    },
    Update {
        table: String,
        old_row: HashMap<String, Value>,
        new_row: HashMap<String, Value>,
        file_path: String,
    },
    Delete {
        table: String,
        row: HashMap<String, Value>,
        file_path: String,
    },
    Error {
        /// The table whose glob matched the failing file, when known.
        /// `None` for errors that aren't tied to a specific table (e.g.
        /// a watch-channel failure before file attribution).
        table: Option<String>,
        file_path: PathBuf,
        error: String,
    },
}

/// Diff old and new file content to produce minimal row events.
///
/// - `table`: the target table name
/// - `old`: previous row content (None if file is new)
/// - `new`: current row content (None if file was deleted)
/// - `file_path`: the relative file path to attach to each emitted event
///
/// For multi-row files (JSONL), uses line-index-based identity:
/// - Unchanged lines produce no events
/// - Changed lines produce Update events
/// - Additional lines at the end produce Insert events
/// - If the file shrunk or more than half the rows changed, does a full replace
///
/// For single-row files, compares the single row directly.
pub fn diff(
    table: &str,
    old: Option<&[HashMap<String, Value>]>,
    new: Option<&[HashMap<String, Value>]>,
    file_path: &str,
) -> Vec<RowEvent> {
    match (old, new) {
        (None, None) => Vec::new(),
        (None, Some(new_rows)) => new_rows
            .iter()
            .map(|r| RowEvent::Insert {
                table: table.to_string(),
                row: r.clone(),
                file_path: file_path.to_string(),
            })
            .collect(),
        (Some(old_rows), None) => old_rows
            .iter()
            .map(|r| RowEvent::Delete {
                table: table.to_string(),
                row: r.clone(),
                file_path: file_path.to_string(),
            })
            .collect(),
        (Some(old_rows), Some(new_rows)) => diff_rows(table, old_rows, new_rows, file_path),
    }
}

/// Compare old and new row slices and produce minimal events.
fn diff_rows(
    table: &str,
    old_rows: &[HashMap<String, Value>],
    new_rows: &[HashMap<String, Value>],
    file_path: &str,
) -> Vec<RowEvent> {
    // If file shrunk, do full replace
    if new_rows.len() < old_rows.len() {
        return full_replace(table, old_rows, new_rows, file_path);
    }

    // Compare overlapping rows line by line
    let overlap = old_rows.len();
    let mut changed = 0;
    let mut events = Vec::new();

    for i in 0..overlap {
        if old_rows[i] != new_rows[i] {
            changed += 1;
        }
    }

    // For multi-row files, if more than half of overlapping rows changed, full replace.
    // Single-row files (overlap == 1) never trigger full replace -- they use Update.
    if overlap > 1 && changed * 2 > overlap {
        return full_replace(table, old_rows, new_rows, file_path);
    }

    // Emit Update events for changed lines
    for i in 0..overlap {
        if old_rows[i] != new_rows[i] {
            events.push(RowEvent::Update {
                table: table.to_string(),
                old_row: old_rows[i].clone(),
                new_row: new_rows[i].clone(),
                file_path: file_path.to_string(),
            });
        }
    }

    // Emit Insert events for appended lines
    for row in &new_rows[overlap..] {
        events.push(RowEvent::Insert {
            table: table.to_string(),
            row: row.clone(),
            file_path: file_path.to_string(),
        });
    }

    events
}

/// Full replace: delete all old rows, then insert all new rows.
fn full_replace(
    table: &str,
    old_rows: &[HashMap<String, Value>],
    new_rows: &[HashMap<String, Value>],
    file_path: &str,
) -> Vec<RowEvent> {
    let mut events = Vec::with_capacity(old_rows.len() + new_rows.len());
    for row in old_rows {
        events.push(RowEvent::Delete {
            table: table.to_string(),
            row: row.clone(),
            file_path: file_path.to_string(),
        });
    }
    for row in new_rows {
        events.push(RowEvent::Insert {
            table: table.to_string(),
            row: row.clone(),
            file_path: file_path.to_string(),
        });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn int(i: i64) -> Value {
        Value::Integer(i)
    }

    // --- All inserts (file created) ---

    #[test]
    fn all_inserts_when_old_is_none() {
        let rows = vec![
            row(&[("name", text("alice")), ("age", int(30))]),
            row(&[("name", text("bob")), ("age", int(25))]),
        ];
        let events = diff("users", None, Some(&rows), "users.jsonl");
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            RowEvent::Insert { table, row, file_path }
                if table == "users" && row["name"] == text("alice") && file_path == "users.jsonl"
        ));
        assert!(matches!(
            &events[1],
            RowEvent::Insert { table, row, file_path }
                if table == "users" && row["name"] == text("bob") && file_path == "users.jsonl"
        ));
    }

    // --- All deletes (file deleted) ---

    #[test]
    fn all_deletes_when_new_is_none() {
        let rows = vec![row(&[("id", text("1"))]), row(&[("id", text("2"))])];
        let events = diff("items", Some(&rows), None, "items.jsonl");
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            RowEvent::Delete { table, row, file_path }
                if table == "items" && row["id"] == text("1") && file_path == "items.jsonl"
        ));
        assert!(matches!(
            &events[1],
            RowEvent::Delete { table, row, file_path }
                if table == "items" && row["id"] == text("2") && file_path == "items.jsonl"
        ));
    }

    // --- No changes ---

    #[test]
    fn no_events_when_content_identical() {
        let rows = vec![row(&[("x", int(1))]), row(&[("x", int(2))])];
        let events = diff("t", Some(&rows), Some(&rows), "t.jsonl");
        assert!(events.is_empty());
    }

    // --- Single line change ---

    #[test]
    fn update_event_for_changed_line() {
        let old = vec![
            row(&[("val", text("a"))]),
            row(&[("val", text("b"))]),
            row(&[("val", text("c"))]),
        ];
        let new = vec![
            row(&[("val", text("a"))]),
            row(&[("val", text("B"))]),
            row(&[("val", text("c"))]),
        ];
        let events = diff("t", Some(&old), Some(&new), "t.jsonl");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            RowEvent::Update { table, old_row, new_row, file_path }
                if table == "t"
                    && old_row["val"] == text("b")
                    && new_row["val"] == text("B")
                    && file_path == "t.jsonl"
        ));
    }

    // --- Append new lines ---

    #[test]
    fn insert_events_for_appended_lines() {
        let old = vec![row(&[("id", int(1))])];
        let new = vec![
            row(&[("id", int(1))]),
            row(&[("id", int(2))]),
            row(&[("id", int(3))]),
        ];
        let events = diff("t", Some(&old), Some(&new), "t.jsonl");
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            RowEvent::Insert { table, row, file_path }
                if table == "t" && row["id"] == int(2) && file_path == "t.jsonl"
        ));
        assert!(matches!(
            &events[1],
            RowEvent::Insert { table, row, file_path }
                if table == "t" && row["id"] == int(3) && file_path == "t.jsonl"
        ));
    }

    // --- Full replace on shrink ---

    #[test]
    fn full_replace_when_file_shrinks() {
        let old = vec![
            row(&[("id", int(1))]),
            row(&[("id", int(2))]),
            row(&[("id", int(3))]),
        ];
        let new = vec![row(&[("id", int(1))])];
        let events = diff("t", Some(&old), Some(&new), "t.jsonl");
        // Should be 3 deletes + 1 insert = 4 events
        assert_eq!(events.len(), 4);
        let deletes: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RowEvent::Delete { .. }))
            .collect();
        let inserts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RowEvent::Insert { .. }))
            .collect();
        assert_eq!(deletes.len(), 3);
        assert_eq!(inserts.len(), 1);
        assert!(events.iter().all(|e| match e {
            RowEvent::Insert { file_path, .. }
            | RowEvent::Update { file_path, .. }
            | RowEvent::Delete { file_path, .. } => file_path == "t.jsonl",
            RowEvent::Error { .. } => false,
        }));
    }

    // --- Full replace on heavy modification ---

    #[test]
    fn full_replace_when_more_than_half_changed() {
        let old = vec![
            row(&[("v", text("a"))]),
            row(&[("v", text("b"))]),
            row(&[("v", text("c"))]),
            row(&[("v", text("d"))]),
        ];
        // 3 out of 4 changed = 75% > 50%, triggers full replace
        let new = vec![
            row(&[("v", text("A"))]),
            row(&[("v", text("B"))]),
            row(&[("v", text("C"))]),
            row(&[("v", text("d"))]),
        ];
        let events = diff("t", Some(&old), Some(&new), "t.jsonl");
        let deletes: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RowEvent::Delete { .. }))
            .collect();
        let inserts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RowEvent::Insert { .. }))
            .collect();
        // Full replace: 4 deletes + 4 inserts
        assert_eq!(deletes.len(), 4);
        assert_eq!(inserts.len(), 4);
    }

    // --- Single-row file: update ---

    #[test]
    fn single_row_update() {
        let old = vec![row(&[("title", text("Draft"))])];
        let new = vec![row(&[("title", text("Final"))])];
        let events = diff("docs", Some(&old), Some(&new), "doc.json");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            RowEvent::Update { table, old_row, new_row, file_path }
                if table == "docs"
                    && old_row["title"] == text("Draft")
                    && new_row["title"] == text("Final")
                    && file_path == "doc.json"
        ));
    }

    // --- Single-row file: no change ---

    #[test]
    fn single_row_no_change() {
        let rows = vec![row(&[("title", text("Same"))])];
        let events = diff("docs", Some(&rows), Some(&rows), "doc.json");
        assert!(events.is_empty());
    }

    // --- Both None ---

    #[test]
    fn no_events_when_both_none() {
        let events = diff("t", None, None, "gone.json");
        assert!(events.is_empty());
    }

    // --- Exactly half changed should NOT trigger full replace ---

    #[test]
    fn no_full_replace_when_exactly_half_changed() {
        let old = vec![
            row(&[("v", text("a"))]),
            row(&[("v", text("b"))]),
            row(&[("v", text("c"))]),
            row(&[("v", text("d"))]),
        ];
        // 2 out of 4 changed = 50%, should NOT trigger full replace
        let new = vec![
            row(&[("v", text("A"))]),
            row(&[("v", text("B"))]),
            row(&[("v", text("c"))]),
            row(&[("v", text("d"))]),
        ];
        let events = diff("t", Some(&old), Some(&new), "t.jsonl");
        // Should be 2 Update events, not a full replace
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| matches!(e, RowEvent::Update { .. })));
    }

    // --- Full replace: deletes come before inserts ---

    #[test]
    fn full_replace_deletes_before_inserts() {
        let old = vec![row(&[("id", int(1))]), row(&[("id", int(2))])];
        let new = vec![row(&[("id", int(3))])];
        let events = diff("t", Some(&old), Some(&new), "t.jsonl");
        // Find the index of the last delete and first insert
        let last_delete = events
            .iter()
            .rposition(|e| matches!(e, RowEvent::Delete { .. }));
        let first_insert = events
            .iter()
            .position(|e| matches!(e, RowEvent::Insert { .. }));
        assert!(
            last_delete.unwrap() < first_insert.unwrap(),
            "Deletes should come before inserts in full replace"
        );
    }
}
