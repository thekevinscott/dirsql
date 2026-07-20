//! Schema inference from row-object output.

use crate::Value;

/// A SQLite column type a row-object key can be inferred to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    Text,
    Integer,
    Real,
}

/// One inferred column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: SqlType,
}

/// A row object with its key order preserved as the parser emitted it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JsonRow(pub Vec<(String, serde_json::Value)>);

/// Parse a parser command's payload into ordered row objects.
pub fn parse_rows(_payload: &str) -> Result<Vec<JsonRow>, String> {
    Ok(Vec::new())
}

/// Infer a column list from sampled row objects.
pub fn infer_schema(_rows: &[JsonRow]) -> Vec<Column> {
    Vec::new()
}

/// The value of `column` in `row`, NULL when the key is absent.
pub fn cell(_row: &JsonRow, _column: &str) -> Value {
    Value::Null
}
