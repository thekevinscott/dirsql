//! Query engine abstraction. The HTTP layer depends on [`QueryEngine`] so it
//! can be driven with either a real [`DirSQL`] or a [`MockEngine`] in tests.

use crate::error::QueryError;
use base64::Engine as _;
use dirsql_core::db::Value;
use dirsql_sdk::DirSQL;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub type Row = HashMap<String, Value>;

pub trait QueryEngine: Send + Sync {
    fn query(&self, sql: &str) -> Result<Vec<Row>, QueryError>;
}

/// Wraps a real [`DirSQL`] instance.
pub struct DirSQLEngine {
    inner: DirSQL,
}

impl DirSQLEngine {
    /// Build from a config file path (e.g. `/path/to/.dirsql.toml`).
    ///
    /// The SDK's `from_config` takes a *directory* containing a
    /// `.dirsql.toml`. If the caller passed a full config file path, we
    /// support that by materializing the config file in a chosen directory
    /// layout. For the v1 CLI we accept two shapes:
    ///   - config path ends in `.dirsql.toml` and lives inside `root` --
    ///     use `root`.
    ///   - otherwise: temporarily create a symlink/copy in a side directory
    ///     is out of scope; we instead copy the config to `<root>/.dirsql.toml`
    ///     if one is not already there.
    pub fn from_config_path(root: &Path, config: Option<&Path>) -> Result<Self, QueryError> {
        let root_buf: PathBuf = root.to_path_buf();
        let default_conf = root_buf.join(".dirsql.toml");
        let effective_dir = match config {
            None => root_buf,
            Some(cp) => {
                // If the config is already at <root>/.dirsql.toml, use root as-is.
                if cp == default_conf.as_path() {
                    root_buf
                } else {
                    // Copy user-provided config to root/.dirsql.toml only if
                    // one doesn't already exist (don't clobber).
                    if !default_conf.exists() {
                        std::fs::copy(cp, &default_conf)
                            .map_err(|e| QueryError::Engine(format!("copy config: {e}")))?;
                    } else if std::fs::read(cp).ok() != std::fs::read(&default_conf).ok() {
                        // Config already present but differs from requested.
                        // Prefer the explicit flag: overwrite.
                        std::fs::copy(cp, &default_conf)
                            .map_err(|e| QueryError::Engine(format!("overwrite config: {e}")))?;
                    }
                    root_buf
                }
            }
        };

        let inner =
            DirSQL::from_config(effective_dir).map_err(|e| QueryError::Engine(e.to_string()))?;
        Ok(Self { inner })
    }
}

impl QueryEngine for DirSQLEngine {
    fn query(&self, sql: &str) -> Result<Vec<Row>, QueryError> {
        self.inner
            .query(sql)
            .map_err(|e| QueryError::Engine(e.to_string()))
    }
}

/// Mock engine for tests. Records the last SQL seen and returns either a
/// canned result or a canned error.
pub struct MockEngine {
    response: Result<Vec<Row>, String>,
    last: Mutex<Option<String>>,
}

impl MockEngine {
    pub fn with_rows(rows: Vec<Row>) -> Self {
        Self {
            response: Ok(rows),
            last: Mutex::new(None),
        }
    }

    pub fn with_error(msg: impl Into<String>) -> Self {
        Self {
            response: Err(msg.into()),
            last: Mutex::new(None),
        }
    }

    pub fn last_sql(&self) -> Option<String> {
        self.last.lock().unwrap().clone()
    }
}

impl QueryEngine for MockEngine {
    fn query(&self, sql: &str) -> Result<Vec<Row>, QueryError> {
        *self.last.lock().unwrap() = Some(sql.to_string());
        match &self.response {
            Ok(rows) => Ok(rows.clone()),
            Err(m) => Err(QueryError::Engine(m.clone())),
        }
    }
}

/// Convert a core [`Value`] into a `serde_json::Value`. Blobs are base64-encoded.
pub fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Integer(i) => serde_json::Value::Number((*i).into()),
        Value::Real(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Blob(b) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(b);
            serde_json::json!({ "$blob_b64": encoded })
        }
    }
}

pub fn row_to_json(row: &Row) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = row
        .iter()
        .map(|(k, v)| (k.clone(), value_to_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_engine_records_sql() {
        let m = MockEngine::with_rows(vec![]);
        m.query("SELECT 1").unwrap();
        assert_eq!(m.last_sql(), Some("SELECT 1".into()));
    }

    #[test]
    fn mock_engine_returns_error() {
        let m = MockEngine::with_error("err");
        let r = m.query("x");
        assert!(matches!(r, Err(QueryError::Engine(_))));
    }

    #[test]
    fn value_to_json_all_variants() {
        assert_eq!(value_to_json(&Value::Null), serde_json::Value::Null);
        assert_eq!(value_to_json(&Value::Integer(7)), serde_json::json!(7));
        let f = value_to_json(&Value::Real(1.5));
        assert_eq!(f, serde_json::json!(1.5));
        assert_eq!(
            value_to_json(&Value::Text("x".into())),
            serde_json::json!("x")
        );
        let b = value_to_json(&Value::Blob(vec![0, 1, 2]));
        assert!(b.get("$blob_b64").is_some());
        // non-finite floats collapse to null
        let nan = value_to_json(&Value::Real(f64::NAN));
        assert_eq!(nan, serde_json::Value::Null);
    }

    #[test]
    fn row_to_json_roundtrip() {
        let mut r: Row = HashMap::new();
        r.insert("a".into(), Value::Integer(1));
        r.insert("b".into(), Value::Text("y".into()));
        let j = row_to_json(&r);
        assert_eq!(j["a"], serde_json::json!(1));
        assert_eq!(j["b"], serde_json::json!("y"));
    }
}
