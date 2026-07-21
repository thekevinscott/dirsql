//! Schema inference from row-object output.
//!
//! A parser's contract is a JSON array of row objects. This module turns a
//! sample of those rows into a SQLite column list, so a table can exist
//! without the user writing DDL for it.
//!
//! ## Rules
//!
//! - **Columns are the union of keys** across every sampled row, in
//!   **first-seen order** — the order the parser emitted them, not sorted
//!   order — so `SELECT *` is stable across runs.
//! - **Types** come from the JSON values: string → `TEXT`, integer →
//!   `INTEGER`, float → `REAL`, bool → `INTEGER`, nested object/array → its
//!   JSON text as `TEXT`.
//! - **Null and missing carry no type.** A key that is null in one row takes
//!   its type from the rows where it isn't; a key that is never non-null is
//!   `TEXT`.
//! - **Disagreement is `TEXT`.** Any two rows claiming different types for one
//!   key fall back to `TEXT`, SQLite's most forgiving affinity. That includes
//!   integer vs. float: widening to `REAL` would be a second rule to remember
//!   for no gain SQLite's affinity doesn't already provide.
//!
//! Everything here is pure. Reading files, running parsers, and declaring the
//! schema to SQLite belong to the caller (see [`crate::parsed_vtab`]).

use std::collections::HashMap;
use std::fmt;

use serde::de::{Deserializer, MapAccess, Visitor};

use crate::Value;
use crate::json_to_value;

/// A SQLite column type a row-object key can be inferred to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    Text,
    Integer,
    Real,
}

impl SqlType {
    /// The keyword as it appears in DDL.
    pub fn as_str(self) -> &'static str {
        match self {
            SqlType::Text => "TEXT",
            SqlType::Integer => "INTEGER",
            SqlType::Real => "REAL",
        }
    }
}

/// One inferred column: a key and the type its values agreed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: SqlType,
}

/// A row object with its key order preserved as the parser emitted it.
///
/// `serde_json::Map` sorts its keys, which would make column order
/// alphabetical; keeping the pairs in a `Vec` is what makes first-seen
/// ordering possible at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JsonRow(pub Vec<(String, serde_json::Value)>);

impl JsonRow {
    /// The raw JSON value for `key`, or `None` when the row omits it.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

impl<'de> serde::Deserialize<'de> for JsonRow {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RowVisitor;

        impl<'de> Visitor<'de> for RowVisitor {
            type Value = JsonRow;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON row object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonRow, A::Error> {
                let mut pairs = Vec::new();
                while let Some(pair) = map.next_entry::<String, serde_json::Value>()? {
                    pairs.push(pair);
                }
                Ok(JsonRow(pairs))
            }
        }

        deserializer.deserialize_map(RowVisitor)
    }
}

/// Parse a parser command's payload — a JSON array of row objects — into rows
/// that remember their key order.
pub fn parse_rows(payload: &str) -> Result<Vec<JsonRow>, String> {
    serde_json::from_str::<Vec<JsonRow>>(payload)
        .map_err(|e| format!("expected a JSON array of row objects: {e}"))
}

/// What the rows have said about one key so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Observed {
    /// Seen, but only as null or missing — no type yet.
    Untyped,
    Typed(SqlType),
    /// Two rows disagreed.
    Conflict,
}

impl Observed {
    /// Fold one more value in. Null contributes nothing, a first type wins, a
    /// matching type is a no-op, and a differing type is a conflict.
    fn observe(self, value: &serde_json::Value) -> Self {
        let Some(ty) = value_type(value) else {
            return self;
        };
        match self {
            Observed::Untyped => Observed::Typed(ty),
            Observed::Typed(seen) if seen == ty => self,
            Observed::Typed(_) | Observed::Conflict => Observed::Conflict,
        }
    }

    /// The column type this resolves to. Both "never typed" and "disagreed"
    /// land on `TEXT`.
    fn resolve(self) -> SqlType {
        match self {
            Observed::Typed(ty) => ty,
            Observed::Untyped | Observed::Conflict => SqlType::Text,
        }
    }
}

/// The type a single JSON value implies, or `None` for null (which types
/// nothing). Mirrors [`json_to_value`]'s mapping, so a column's declared type
/// always matches the values stored under it.
fn value_type(value: &serde_json::Value) -> Option<SqlType> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(_) => Some(SqlType::Integer),
        serde_json::Value::Number(n) => Some(if n.is_i64() {
            SqlType::Integer
        } else {
            SqlType::Real
        }),
        // Strings, and nested objects/arrays stored as their JSON text.
        _ => Some(SqlType::Text),
    }
}

/// Infer the column list for `rows`: the union of their keys in first-seen
/// order, each typed by every value that appeared under it.
pub fn infer_schema(rows: &[JsonRow]) -> Vec<Column> {
    let mut order: Vec<String> = Vec::new();
    let mut observed: HashMap<&str, Observed> = HashMap::new();

    for row in rows {
        for (key, value) in &row.0 {
            let entry = observed.entry(key.as_str()).or_insert_with(|| {
                order.push(key.clone());
                Observed::Untyped
            });
            *entry = entry.observe(value);
        }
    }

    order
        .into_iter()
        .map(|name| {
            let ty = observed[name.as_str()].resolve();
            Column { name, ty }
        })
        .collect()
}

/// The value of `column` in `row` — NULL when the row omits the key, which is
/// how a union schema stays rectangular over ragged rows.
pub fn cell(row: &JsonRow, column: &str) -> Value {
    row.get(column).map(json_to_value).unwrap_or(Value::Null)
}

/// The `CREATE TABLE` statement declaring `columns`, for a vtab's schema.
/// Names are quoted, so a key that collides with a SQL keyword or carries
/// punctuation still declares cleanly.
pub fn declared_schema(columns: &[Column]) -> String {
    let declarations = columns
        .iter()
        .map(|c| format!("{} {}", quote_identifier(&c.name), c.ty.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CREATE TABLE x({declarations})")
}

/// Quote `name` as a SQL identifier, doubling any embedded quote.
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(payload: &str) -> Vec<JsonRow> {
        parse_rows(payload).expect("valid payload")
    }

    fn names(columns: &[Column]) -> Vec<&str> {
        columns.iter().map(|c| c.name.as_str()).collect()
    }

    fn ty_of(columns: &[Column], name: &str) -> SqlType {
        columns
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no column {name} in {columns:?}"))
            .ty
    }

    #[test]
    fn sql_type_renders_its_ddl_keyword() {
        assert_eq!(SqlType::Text.as_str(), "TEXT");
        assert_eq!(SqlType::Integer.as_str(), "INTEGER");
        assert_eq!(SqlType::Real.as_str(), "REAL");
    }

    #[test]
    fn parse_rows_preserves_key_order_rather_than_sorting() {
        let parsed = rows(r#"[{"zeta":1,"alpha":2}]"#);
        assert_eq!(
            parsed[0]
                .0
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["zeta", "alpha"]
        );
    }

    #[test]
    fn parse_rows_reads_every_element() {
        assert_eq!(rows(r#"[{"a":1},{"b":2}]"#).len(), 2);
    }

    #[test]
    fn parse_rows_accepts_an_empty_array() {
        assert_eq!(rows("[]"), Vec::<JsonRow>::new());
    }

    #[test]
    fn parse_rows_rejects_a_non_array_payload() {
        let err = parse_rows(r#"{"a":1}"#).unwrap_err();
        assert!(err.contains("array of row objects"), "got: {err}");
    }

    #[test]
    fn parse_rows_rejects_an_element_that_is_not_an_object() {
        let err = parse_rows("[3]").unwrap_err();
        assert!(err.contains("array of row objects"), "got: {err}");
    }

    #[test]
    fn parse_rows_rejects_invalid_json() {
        assert!(parse_rows("not json").is_err());
    }

    #[test]
    fn json_row_get_finds_a_key_and_misses_an_absent_one() {
        let row = &rows(r#"[{"a":1}]"#)[0];
        assert_eq!(row.get("a"), Some(&serde_json::json!(1)));
        assert_eq!(row.get("b"), None);
    }

    #[test]
    fn json_row_default_is_empty() {
        assert_eq!(JsonRow::default().0.len(), 0);
    }

    #[test]
    fn infer_schema_of_no_rows_is_no_columns() {
        assert_eq!(infer_schema(&[]), Vec::new());
    }

    #[test]
    fn infer_schema_of_a_row_with_no_keys_is_no_columns() {
        assert_eq!(infer_schema(&rows("[{}]")), Vec::new());
    }

    #[test]
    fn columns_are_the_union_of_keys_across_rows() {
        let columns = infer_schema(&rows(r#"[{"a":1},{"b":2},{"a":3,"c":4}]"#));
        assert_eq!(names(&columns), vec!["a", "b", "c"]);
    }

    #[test]
    fn column_order_is_first_seen_not_alphabetical() {
        let columns = infer_schema(&rows(r#"[{"zeta":1,"alpha":2},{"middle":3}]"#));
        assert_eq!(names(&columns), vec!["zeta", "alpha", "middle"]);
    }

    #[test]
    fn a_repeated_key_does_not_repeat_its_column() {
        let columns = infer_schema(&rows(r#"[{"a":1},{"a":2},{"a":3}]"#));
        assert_eq!(names(&columns), vec!["a"]);
    }

    #[test]
    fn a_string_is_text() {
        let columns = infer_schema(&rows(r#"[{"s":"x"}]"#));
        assert_eq!(ty_of(&columns, "s"), SqlType::Text);
    }

    #[test]
    fn an_integer_is_integer() {
        let columns = infer_schema(&rows(r#"[{"i":42}]"#));
        assert_eq!(ty_of(&columns, "i"), SqlType::Integer);
    }

    #[test]
    fn a_negative_integer_is_integer() {
        let columns = infer_schema(&rows(r#"[{"i":-42}]"#));
        assert_eq!(ty_of(&columns, "i"), SqlType::Integer);
    }

    #[test]
    fn a_float_is_real() {
        let columns = infer_schema(&rows(r#"[{"f":1.5}]"#));
        assert_eq!(ty_of(&columns, "f"), SqlType::Real);
    }

    #[test]
    fn a_number_too_large_for_i64_is_real() {
        // 10^19 exceeds i64::MAX, matching `json_to_value`'s fallback to Real.
        let columns = infer_schema(&rows(r#"[{"big":10000000000000000000}]"#));
        assert_eq!(ty_of(&columns, "big"), SqlType::Real);
    }

    #[test]
    fn a_bool_is_integer() {
        let columns = infer_schema(&rows(r#"[{"t":true},{"t":false}]"#));
        assert_eq!(ty_of(&columns, "t"), SqlType::Integer);
    }

    #[test]
    fn a_nested_object_is_text() {
        let columns = infer_schema(&rows(r#"[{"obj":{"k":"v"}}]"#));
        assert_eq!(ty_of(&columns, "obj"), SqlType::Text);
    }

    #[test]
    fn a_nested_array_is_text() {
        let columns = infer_schema(&rows(r#"[{"arr":[1,2]}]"#));
        assert_eq!(ty_of(&columns, "arr"), SqlType::Text);
    }

    #[test]
    fn a_key_that_is_never_non_null_is_text() {
        let columns = infer_schema(&rows(r#"[{"n":null},{"n":null}]"#));
        assert_eq!(ty_of(&columns, "n"), SqlType::Text);
    }

    #[test]
    fn a_null_takes_its_type_from_a_later_row() {
        let columns = infer_schema(&rows(r#"[{"n":null},{"n":7}]"#));
        assert_eq!(ty_of(&columns, "n"), SqlType::Integer);
    }

    #[test]
    fn a_null_does_not_erase_a_type_seen_earlier() {
        let columns = infer_schema(&rows(r#"[{"n":7},{"n":null}]"#));
        assert_eq!(ty_of(&columns, "n"), SqlType::Integer);
    }

    #[test]
    fn a_key_missing_from_a_row_still_takes_its_type_from_the_others() {
        let columns = infer_schema(&rows(r#"[{"a":1,"b":"x"},{"a":2}]"#));
        assert_eq!(ty_of(&columns, "b"), SqlType::Text);
    }

    #[test]
    fn conflicting_types_fall_back_to_text() {
        let columns = infer_schema(&rows(r#"[{"m":1},{"m":"two"}]"#));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn an_integer_and_a_float_conflict_rather_than_widening() {
        let columns = infer_schema(&rows(r#"[{"m":1},{"m":1.5}]"#));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn a_conflict_survives_further_agreeing_rows() {
        let columns = infer_schema(&rows(r#"[{"m":1},{"m":"two"},{"m":3}]"#));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn a_conflict_survives_a_later_null() {
        let columns = infer_schema(&rows(r#"[{"m":1},{"m":"two"},{"m":null}]"#));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn a_bool_and_an_integer_agree_on_integer() {
        let columns = infer_schema(&rows(r#"[{"m":true},{"m":3}]"#));
        assert_eq!(ty_of(&columns, "m"), SqlType::Integer);
    }

    #[test]
    fn value_type_is_none_only_for_null() {
        assert_eq!(value_type(&serde_json::Value::Null), None);
        assert_eq!(value_type(&serde_json::json!("s")), Some(SqlType::Text));
    }

    #[test]
    fn observed_starts_untyped_and_resolves_to_text() {
        assert_eq!(Observed::Untyped.resolve(), SqlType::Text);
    }

    #[test]
    fn observed_conflict_resolves_to_text() {
        assert_eq!(Observed::Conflict.resolve(), SqlType::Text);
    }

    #[test]
    fn observed_typed_resolves_to_its_type() {
        assert_eq!(Observed::Typed(SqlType::Real).resolve(), SqlType::Real);
    }

    #[test]
    fn observing_null_leaves_the_state_untouched() {
        let null = serde_json::Value::Null;
        assert_eq!(Observed::Untyped.observe(&null), Observed::Untyped);
        assert_eq!(
            Observed::Typed(SqlType::Text).observe(&null),
            Observed::Typed(SqlType::Text)
        );
        assert_eq!(Observed::Conflict.observe(&null), Observed::Conflict);
    }

    #[test]
    fn observing_a_matching_type_is_a_no_op() {
        assert_eq!(
            Observed::Typed(SqlType::Integer).observe(&serde_json::json!(2)),
            Observed::Typed(SqlType::Integer)
        );
    }

    #[test]
    fn observing_a_differing_type_conflicts() {
        assert_eq!(
            Observed::Typed(SqlType::Integer).observe(&serde_json::json!("s")),
            Observed::Conflict
        );
    }

    #[test]
    fn cell_reads_a_present_key() {
        let row = &rows(r#"[{"s":"x","i":1}]"#)[0];
        assert_eq!(cell(row, "s"), Value::Text("x".into()));
        assert_eq!(cell(row, "i"), Value::Integer(1));
    }

    #[test]
    fn cell_is_null_for_a_missing_key() {
        let row = &rows(r#"[{"a":1}]"#)[0];
        assert_eq!(cell(row, "absent"), Value::Null);
    }

    #[test]
    fn cell_is_null_for_a_null_value() {
        let row = &rows(r#"[{"a":null}]"#)[0];
        assert_eq!(cell(row, "a"), Value::Null);
    }

    #[test]
    fn cell_renders_nested_values_as_json_text() {
        let row = &rows(r#"[{"obj":{"k":"v"},"arr":[1,2]}]"#)[0];
        assert_eq!(cell(row, "obj"), Value::Text(r#"{"k":"v"}"#.into()));
        assert_eq!(cell(row, "arr"), Value::Text("[1,2]".into()));
    }

    #[test]
    fn declared_schema_lists_every_column_with_its_type() {
        let columns = infer_schema(&rows(r#"[{"s":"x","i":1,"f":1.5}]"#));
        assert_eq!(
            declared_schema(&columns),
            r#"CREATE TABLE x("s" TEXT, "i" INTEGER, "f" REAL)"#
        );
    }

    #[test]
    fn declared_schema_quotes_a_keyword_column() {
        let columns = infer_schema(&rows(r#"[{"select":1}]"#));
        assert_eq!(
            declared_schema(&columns),
            r#"CREATE TABLE x("select" INTEGER)"#
        );
    }

    #[test]
    fn quote_identifier_doubles_an_embedded_quote() {
        assert_eq!(quote_identifier(r#"a"b"#), r#""a""b""#);
    }
}
