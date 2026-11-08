//! Schema inference from row-object output.
//!
//! A parser's contract is one JSON row object per line. This module turns a
//! sample of those rows into a SQLite column list, so a table can exist
//! without the user writing DDL for it.
//!
//! ## Rules
//!
//! - **Columns are the union of keys** across every sampled row, in
//!   **first-seen order** — the order the parser emitted them, not sorted
//!   order — so `SELECT *` is stable across runs.
//! - **Keys differing only in ASCII case are one column**, named by the
//!   first-seen spelling, since SQLite identifiers are case-insensitive and
//!   could not declare both.
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

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use serde::de::{DeserializeSeed, Deserializer, MapAccess, Visitor};

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
///
/// Keys are shared: every row parsed from one block of output holds the same
/// allocation for a key, so millions of rows cost no per-row key strings to
/// build or to free on the thread that stores them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JsonRow(pub Vec<(Arc<str>, serde_json::Value)>);

impl JsonRow {
    /// The raw JSON value for `key`, or `None` when the row omits it.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.iter().find(|(k, _)| **k == *key).map(|(_, v)| v)
    }
}

impl serde::Serialize for JsonRow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(&**key, value)?;
        }
        map.end()
    }
}

/// The keys one block of output has used, handed out shared.
#[derive(Default)]
struct Keys(HashSet<Arc<str>>);

impl Keys {
    fn intern(&mut self, key: &str) -> Arc<str> {
        if let Some(seen) = self.0.get(key) {
            return Arc::clone(seen);
        }
        let key: Arc<str> = Arc::from(key);
        self.0.insert(Arc::clone(&key));
        key
    }
}

/// A key as the line spells it, borrowed unless it carries an escape.
#[derive(serde::Deserialize)]
struct Key<'a>(#[serde(borrow)] Cow<'a, str>);

/// Deserializes one row, drawing its keys from a block's [`Keys`].
struct RowSeed<'k>(&'k mut Keys);

impl<'de> DeserializeSeed<'de> for RowSeed<'_> {
    type Value = JsonRow;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<JsonRow, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for RowSeed<'_> {
    type Value = JsonRow;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON row object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonRow, A::Error> {
        let mut pairs: Vec<(Arc<str>, serde_json::Value)> = Vec::new();
        while let Some(Key(key)) = map.next_key::<Key<'de>>()? {
            let key = self.0.intern(&key);
            let value = map.next_value::<serde_json::Value>()?;
            // A repeated key keeps its last value, as jq and JSON.parse read it;
            // interned keys make the lookup a pointer compare.
            match pairs.iter_mut().find(|(seen, _)| Arc::ptr_eq(seen, &key)) {
                Some((_, slot)) => *slot = value,
                None => pairs.push((key, value)),
            }
        }
        Ok(JsonRow(pairs))
    }
}

/// Parse a parser command's output — one JSON object per line (NDJSON), blank
/// lines skipped — into rows that remember their key order.
pub fn parse_rows(output: &str) -> Result<Vec<JsonRow>, String> {
    parse_rows_at(output, 0)
}

/// [`parse_rows`] for a block that begins after `skipped` earlier lines of the
/// same output, so an error names its line in the whole output.
pub(crate) fn parse_rows_at(output: &str, skipped: usize) -> Result<Vec<JsonRow>, String> {
    let mut keys = Keys::default();
    let mut rows = Vec::new();
    for (index, line) in output.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let number = skipped + index + 1;
        if line.starts_with('[') {
            return Err(format!(
                "line {number} is a JSON array; print one JSON object per line instead"
            ));
        }
        let row = parse_row(line, &mut keys).map_err(|e| match lone_surrogate(line) {
            Some(escape) => format!("line {number}: lone surrogate {escape} is not valid Unicode"),
            None => format!("line {number} is not a JSON object: {e}"),
        })?;
        rows.push(row);
    }
    Ok(rows)
}

/// One line as a row, rejecting anything after the object as serde_json's
/// `from_str` does.
fn parse_row(line: &str, keys: &mut Keys) -> serde_json::Result<JsonRow> {
    let mut deserializer = serde_json::Deserializer::from_str(line);
    let mut row = RowSeed(keys).deserialize(&mut deserializer)?;
    deserializer.end()?;
    restore_integral_zeros(line, &mut row)?;
    Ok(row)
}

/// serde_json parses both `-0` and `-0.0` to the float -0.0, so the value
/// alone cannot say whether the line spelled an integer.
fn is_negative_zero(value: &serde_json::Value) -> bool {
    value
        .as_number()
        .and_then(serde_json::Number::as_f64)
        .is_some_and(|f| f == 0.0 && f.is_sign_negative())
}

/// Turn each -0.0 the line spelled without a fraction or exponent back into
/// the integer 0, as SQLite's JSON functions type it. The line is re-read only
/// when a row holds a negative zero.
fn restore_integral_zeros(line: &str, row: &mut JsonRow) -> serde_json::Result<()> {
    let mut spelled: Option<SpelledValues<'_>> = None;
    for (index, (_, value)) in row.0.iter_mut().enumerate() {
        if !is_negative_zero(value) {
            continue;
        }
        let spelled = match &spelled {
            Some(spelled) => spelled,
            None => spelled.insert(serde_json::from_str(line)?),
        };
        if !spelled.0[index].get().contains(['.', 'e', 'E']) {
            *value = serde_json::Value::from(0);
        }
    }
    Ok(())
}

/// A row object's values as the line spells them, in line order.
struct SpelledValues<'a>(Vec<&'a serde_json::value::RawValue>);

impl<'de> serde::Deserialize<'de> for SpelledValues<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(SpelledValues(Vec::new()))
    }
}

impl<'de> Visitor<'de> for SpelledValues<'de> {
    type Value = SpelledValues<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON row object")
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Self, A::Error> {
        while let Some((serde::de::IgnoredAny, raw)) = map.next_entry()? {
            self.0.push(raw);
        }
        Ok(self)
    }
}

/// The first `\uXXXX` escape in `line` naming a UTF-16 surrogate that is not
/// half of a high-low pair, spelled as the line spells it. JSON's grammar
/// admits it, but it names no Unicode scalar value, so no string can hold it.
fn lone_surrogate(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let unit = |at: usize| -> Option<u16> {
        let escape = line.get(at..at + 6)?;
        let digits = escape.strip_prefix("\\u")?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u16::from_str_radix(digits, 16).ok()
    };
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
        let Some(code) = unit(at) else {
            at += 2;
            continue;
        };
        match code {
            0xD800..=0xDBFF if matches!(unit(at + 6), Some(0xDC00..=0xDFFF)) => at += 12,
            0xD800..=0xDFFF => return Some(&line[at..at + 6]),
            _ => at += 6,
        }
    }
    None
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
    let mut columns: Vec<(String, Observed)> = Vec::new();
    // Exact spellings resolve without folding, so the per-cell cost of case
    // insensitivity is paid once per distinct spelling, not once per row.
    let mut by_spelling: HashMap<&str, usize> = HashMap::new();
    let mut by_folded: HashMap<String, usize> = HashMap::new();

    for row in rows {
        for (key, value) in &row.0 {
            let index = *by_spelling.entry(&**key).or_insert_with(|| {
                *by_folded
                    .entry(key.to_ascii_lowercase())
                    .or_insert_with(|| {
                        columns.push((key.to_string(), Observed::Untyped));
                        columns.len() - 1
                    })
            });
            let observed = &mut columns[index].1;
            *observed = observed.observe(value);
        }
    }

    columns
        .into_iter()
        .map(|(name, observed)| Column {
            name,
            ty: observed.resolve(),
        })
        .collect()
}

/// The value of `column` in `row`, matching the key ASCII-case-insensitively
/// as [`infer_schema`] grouped it — NULL when the row omits the key, which is
/// how a union schema stays rectangular over ragged rows.
pub fn cell(row: &JsonRow, column: &str) -> Value {
    row.0
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(column))
        .map(|(_, value)| json_to_value(value))
        .unwrap_or(Value::Null)
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

    fn rows(lines: &[&str]) -> Vec<JsonRow> {
        parse_rows(&lines.join("\n")).expect("valid payload")
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
    fn parse_rows_at_numbers_lines_after_the_skipped_ones() {
        let err = parse_rows_at("{\"a\":1}\nnope", 40).unwrap_err();
        assert!(err.starts_with("line 42 "), "got: {err}");
    }

    #[test]
    fn parse_rows_preserves_key_order_rather_than_sorting() {
        let parsed = parse_rows(r#"{"zeta":1,"alpha":2}"#).unwrap();
        assert_eq!(
            parsed[0].0.iter().map(|(k, _)| &**k).collect::<Vec<_>>(),
            vec!["zeta", "alpha"]
        );
    }

    #[test]
    fn parse_rows_reads_every_line_skipping_blanks() {
        assert_eq!(parse_rows("{\"a\":1}\n\n  \n{\"b\":2}\n").unwrap().len(), 2);
    }

    #[test]
    fn parse_rows_accepts_empty_output() {
        assert_eq!(parse_rows(""), Ok(Vec::<JsonRow>::new()));
    }

    #[test]
    fn parse_rows_rejects_an_array_naming_the_contract() {
        let err = parse_rows("{\"a\":1}\n[{\"a\":1}]").unwrap_err();
        assert_eq!(
            err,
            "line 2 is a JSON array; print one JSON object per line instead"
        );
    }

    #[test]
    fn parse_rows_rejects_a_line_that_is_not_an_object() {
        let err = parse_rows("3").unwrap_err();
        assert!(err.starts_with("line 1 is not a JSON object"), "got: {err}");
        assert!(err.contains("expected a JSON row object"), "got: {err}");
    }

    #[test]
    fn parse_rows_rejects_anything_after_the_object_on_its_line() {
        let err = parse_rows(r#"{"a":1} {"b":2}"#).unwrap_err();
        assert!(err.starts_with("line 1 is not a JSON object"), "got: {err}");
    }

    #[test]
    fn parse_rows_reads_a_key_spelled_with_an_escape() {
        let row = &rows(&[r#"{"a\u0062":1}"#])[0];
        assert_eq!(row.get("ab"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn parse_rows_keeps_the_last_value_of_a_repeated_key_in_its_first_place() {
        let row = &rows(&[r#"{"k":"first","z":0,"k":"second"}"#])[0];
        assert_eq!(
            row.0
                .iter()
                .map(|(k, v)| (&**k, v.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("k", serde_json::json!("second")),
                ("z", serde_json::json!(0))
            ]
        );
    }

    #[test]
    fn parse_rows_keeps_a_repeated_key_spelled_with_an_escape_once() {
        let row = &rows(&[r#"{"ab":1,"ab":2}"#])[0];
        assert_eq!(row.0.len(), 1);
        assert_eq!(row.get("ab"), Some(&serde_json::json!(2)));
    }

    #[test]
    fn parse_rows_shares_one_allocation_per_key_across_a_blocks_rows() {
        let parsed = rows(&[r#"{"a":1,"b":2}"#, r#"{"b":3,"a":4}"#]);
        let key = |row: usize, i: usize| parsed[row].0[i].0.as_ptr();
        assert_eq!(key(0, 0), key(1, 1));
        assert_eq!(key(0, 1), key(1, 0));
        assert_ne!(key(0, 0), key(0, 1));
    }

    #[test]
    fn parse_rows_names_a_lone_surrogate_and_its_line() {
        let err = parse_rows("{}\n{\"a\":\"\\ud800\"}").unwrap_err();
        assert_eq!(err, r"line 2: lone surrogate \ud800 is not valid Unicode");
    }

    #[test]
    fn parse_rows_turns_only_a_bare_negative_zero_into_integer_zero() {
        let row = &rows(&[r#"{"n":-5,"p":5,"z":-0,"f":-0.0,"e":-0E0,"o":{"z":-0}}"#])[0];
        assert_eq!(row.get("n"), Some(&serde_json::json!(-5)));
        assert_eq!(row.get("p"), Some(&serde_json::json!(5)));
        assert_eq!(row.get("z"), Some(&serde_json::json!(0)));
        assert_eq!(row.get("f"), Some(&serde_json::json!(-0.0)));
        assert_eq!(row.get("e"), Some(&serde_json::json!(-0.0)));
        assert!(row.get("z").unwrap().is_i64());
        assert!(row.get("f").unwrap().is_f64());
    }

    #[test]
    fn spelled_values_expects_a_row_object() {
        let visitor = SpelledValues(Vec::new());
        let expected: &dyn serde::de::Expected = &visitor;
        assert_eq!(format!("{expected}"), "a JSON row object");
    }

    #[test]
    fn lone_surrogate_finds_an_unpaired_leading_or_trailing_half() {
        assert_eq!(lone_surrogate(r#"{"a":"\ud800"}"#), Some(r"\ud800"));
        assert_eq!(lone_surrogate(r#"{"a":"\uDBFF\u0041"}"#), Some(r"\uDBFF"));
        assert_eq!(lone_surrogate(r#"{"a":"x\udc00"}"#), Some(r"\udc00"));
        assert_eq!(lone_surrogate(r#"{"a":"\uDFFF"}"#), Some(r"\uDFFF"));
        assert_eq!(lone_surrogate(r#"{"a":"\ud800"#), Some(r"\ud800"));
    }

    #[test]
    fn lone_surrogate_passes_pairs_other_escapes_and_escaped_backslashes() {
        assert_eq!(lone_surrogate(r#"{"a":"\ud83d\ude00"}"#), None);
        assert_eq!(lone_surrogate(r#"{"a":"\uD7FF\uE000\n\""}"#), None);
        assert_eq!(lone_surrogate(r#"{"a":"\\ud800"}"#), None);
        assert_eq!(lone_surrogate(r#"{"a":"\ud8"}"#), None);
        assert_eq!(lone_surrogate(r#"{"a":"\uzzzz"}"#), None);
        assert_eq!(lone_surrogate(r#"{"a":"\u+d80"}"#), None);
        assert_eq!(lone_surrogate(r#"{"a":"\ud800x\udc00"}"#), Some(r"\ud800"));
        assert_eq!(lone_surrogate("\\"), None);
    }

    #[test]
    fn lone_surrogate_finds_a_lone_half_after_another_escape() {
        assert_eq!(lone_surrogate(r#"{"a":"\u0041\ud800"}"#), Some(r"\ud800"));
    }

    #[test]
    fn lone_surrogate_finds_a_lone_half_after_a_pair() {
        assert_eq!(
            lone_surrogate(r#"{"a":"\ud83d\ude00","b":"\ude00"}"#),
            Some(r"\ude00")
        );
    }

    #[test]
    fn parse_rows_rejects_invalid_json() {
        assert!(parse_rows("not json").is_err());
    }

    #[test]
    fn json_row_get_finds_a_key_and_misses_an_absent_one() {
        let row = &rows(&[r#"{"a":1}"#])[0];
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
        assert_eq!(infer_schema(&rows(&["{}"])), Vec::new());
    }

    #[test]
    fn columns_are_the_union_of_keys_across_rows() {
        let columns = infer_schema(&rows(&[r#"{"a":1}"#, r#"{"b":2}"#, r#"{"a":3,"c":4}"#]));
        assert_eq!(names(&columns), vec!["a", "b", "c"]);
    }

    #[test]
    fn column_order_is_first_seen_not_alphabetical() {
        let columns = infer_schema(&rows(&[r#"{"zeta":1,"alpha":2}"#, r#"{"middle":3}"#]));
        assert_eq!(names(&columns), vec!["zeta", "alpha", "middle"]);
    }

    #[test]
    fn a_repeated_key_does_not_repeat_its_column() {
        let columns = infer_schema(&rows(&[r#"{"a":1}"#, r#"{"a":2}"#, r#"{"a":3}"#]));
        assert_eq!(names(&columns), vec!["a"]);
    }

    #[test]
    fn keys_differing_only_in_case_are_one_column_named_by_the_first_spelling() {
        let columns = infer_schema(&rows(&[r#"{"title":1}"#, r#"{"Title":2,"TITLE":3}"#]));
        assert_eq!(names(&columns), vec!["title"]);
    }

    #[test]
    fn keys_differing_only_in_case_type_one_column_together() {
        let columns = infer_schema(&rows(&[r#"{"m":1}"#, r#"{"M":"two"}"#]));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn keys_differing_beyond_ascii_case_stay_separate_columns() {
        let columns = infer_schema(&rows(&[r#"{"é":1,"É":2}"#]));
        assert_eq!(names(&columns), vec!["é", "É"]);
    }

    #[test]
    fn a_string_is_text() {
        let columns = infer_schema(&rows(&[r#"{"s":"x"}"#]));
        assert_eq!(ty_of(&columns, "s"), SqlType::Text);
    }

    #[test]
    fn an_integer_is_integer() {
        let columns = infer_schema(&rows(&[r#"{"i":42}"#]));
        assert_eq!(ty_of(&columns, "i"), SqlType::Integer);
    }

    #[test]
    fn a_negative_integer_is_integer() {
        let columns = infer_schema(&rows(&[r#"{"i":-42}"#]));
        assert_eq!(ty_of(&columns, "i"), SqlType::Integer);
    }

    #[test]
    fn a_negative_zero_is_integer() {
        let columns = infer_schema(&rows(&[r#"{"z":-0}"#]));
        assert_eq!(ty_of(&columns, "z"), SqlType::Integer);
    }

    #[test]
    fn a_negative_zero_with_a_fraction_or_exponent_is_real() {
        let columns = infer_schema(&rows(&[r#"{"f":-0.0,"e":-0e0}"#]));
        assert_eq!(ty_of(&columns, "f"), SqlType::Real);
        assert_eq!(ty_of(&columns, "e"), SqlType::Real);
    }

    #[test]
    fn a_float_is_real() {
        let columns = infer_schema(&rows(&[r#"{"f":1.5}"#]));
        assert_eq!(ty_of(&columns, "f"), SqlType::Real);
    }

    #[test]
    fn a_number_too_large_for_i64_is_real() {
        // 10^19 exceeds i64::MAX, matching `json_to_value`'s fallback to Real.
        let columns = infer_schema(&rows(&[r#"{"big":10000000000000000000}"#]));
        assert_eq!(ty_of(&columns, "big"), SqlType::Real);
    }

    #[test]
    fn a_bool_is_integer() {
        let columns = infer_schema(&rows(&[r#"{"t":true}"#, r#"{"t":false}"#]));
        assert_eq!(ty_of(&columns, "t"), SqlType::Integer);
    }

    #[test]
    fn a_nested_object_is_text() {
        let columns = infer_schema(&rows(&[r#"{"obj":{"k":"v"}}"#]));
        assert_eq!(ty_of(&columns, "obj"), SqlType::Text);
    }

    #[test]
    fn a_nested_array_is_text() {
        let columns = infer_schema(&rows(&[r#"{"arr":[1,2]}"#]));
        assert_eq!(ty_of(&columns, "arr"), SqlType::Text);
    }

    #[test]
    fn a_key_that_is_never_non_null_is_text() {
        let columns = infer_schema(&rows(&[r#"{"n":null}"#, r#"{"n":null}"#]));
        assert_eq!(ty_of(&columns, "n"), SqlType::Text);
    }

    #[test]
    fn a_null_takes_its_type_from_a_later_row() {
        let columns = infer_schema(&rows(&[r#"{"n":null}"#, r#"{"n":7}"#]));
        assert_eq!(ty_of(&columns, "n"), SqlType::Integer);
    }

    #[test]
    fn a_null_does_not_erase_a_type_seen_earlier() {
        let columns = infer_schema(&rows(&[r#"{"n":7}"#, r#"{"n":null}"#]));
        assert_eq!(ty_of(&columns, "n"), SqlType::Integer);
    }

    #[test]
    fn a_key_missing_from_a_row_still_takes_its_type_from_the_others() {
        let columns = infer_schema(&rows(&[r#"{"a":1,"b":"x"}"#, r#"{"a":2}"#]));
        assert_eq!(ty_of(&columns, "b"), SqlType::Text);
    }

    #[test]
    fn conflicting_types_fall_back_to_text() {
        let columns = infer_schema(&rows(&[r#"{"m":1}"#, r#"{"m":"two"}"#]));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn an_integer_and_a_float_conflict_rather_than_widening() {
        let columns = infer_schema(&rows(&[r#"{"m":1}"#, r#"{"m":1.5}"#]));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn a_conflict_survives_further_agreeing_rows() {
        let columns = infer_schema(&rows(&[r#"{"m":1}"#, r#"{"m":"two"}"#, r#"{"m":3}"#]));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn a_conflict_survives_a_later_null() {
        let columns = infer_schema(&rows(&[r#"{"m":1}"#, r#"{"m":"two"}"#, r#"{"m":null}"#]));
        assert_eq!(ty_of(&columns, "m"), SqlType::Text);
    }

    #[test]
    fn a_bool_and_an_integer_agree_on_integer() {
        let columns = infer_schema(&rows(&[r#"{"m":true}"#, r#"{"m":3}"#]));
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
        let row = &rows(&[r#"{"s":"x","i":1}"#])[0];
        assert_eq!(cell(row, "s"), Value::Text("x".into()));
        assert_eq!(cell(row, "i"), Value::Integer(1));
    }

    #[test]
    fn cell_reads_a_negative_zero_as_integer_zero() {
        let row = &rows(&[r#"{"z":-0,"s":"-0","f":-0.0}"#])[0];
        assert_eq!(cell(row, "z"), Value::Integer(0));
        assert_eq!(cell(row, "s"), Value::Text("-0".into()));
        assert!(matches!(cell(row, "f"), Value::Real(f) if f == 0.0 && f.is_sign_negative()));
    }

    #[test]
    fn cell_reads_a_key_differing_from_the_column_only_in_case() {
        let row = &rows(&[r#"{"Title":"x"}"#])[0];
        assert_eq!(cell(row, "title"), Value::Text("x".into()));
    }

    #[test]
    fn cell_is_null_for_a_missing_key() {
        let row = &rows(&[r#"{"a":1}"#])[0];
        assert_eq!(cell(row, "absent"), Value::Null);
    }

    #[test]
    fn cell_is_null_for_a_null_value() {
        let row = &rows(&[r#"{"a":null}"#])[0];
        assert_eq!(cell(row, "a"), Value::Null);
    }

    #[test]
    fn cell_renders_nested_values_as_json_text() {
        let row = &rows(&[r#"{"obj":{"k":"v"},"arr":[1,2]}"#])[0];
        assert_eq!(cell(row, "obj"), Value::Text(r#"{"k":"v"}"#.into()));
        assert_eq!(cell(row, "arr"), Value::Text("[1,2]".into()));
    }

    #[test]
    fn declared_schema_lists_every_column_with_its_type() {
        let columns = infer_schema(&rows(&[r#"{"s":"x","i":1,"f":1.5}"#]));
        assert_eq!(
            declared_schema(&columns),
            r#"CREATE TABLE x("s" TEXT, "i" INTEGER, "f" REAL)"#
        );
    }

    #[test]
    fn declared_schema_quotes_a_keyword_column() {
        let columns = infer_schema(&rows(&[r#"{"select":1}"#]));
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
