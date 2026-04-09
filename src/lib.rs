pub mod db;
pub mod differ;
pub mod matcher;
pub mod scanner;
pub mod watcher;

/// Extract the table name from a CREATE TABLE DDL statement.
/// Handles: CREATE TABLE name (...), CREATE TABLE IF NOT EXISTS name (...)
pub fn parse_table_name(ddl: &str) -> Option<String> {
    let upper = ddl.to_uppercase();
    let idx = upper.find("CREATE TABLE")?;
    let rest = &ddl[idx + "CREATE TABLE".len()..].trim_start();

    // Skip optional "IF NOT EXISTS"
    let rest = if rest.to_uppercase().starts_with("IF NOT EXISTS") {
        rest["IF NOT EXISTS".len()..].trim_start()
    } else {
        rest
    };

    // Table name is everything up to the first whitespace or '('
    let name: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '(')
        .collect();

    if name.is_empty() { None } else { Some(name) }
}

#[cfg(feature = "extension-module")]
mod python {
    use crate::db::{Db, Value};
    use crate::matcher::TableMatcher;
    use crate::parse_table_name;
    use crate::scanner::scan_directory;
    use pyo3::exceptions::PyRuntimeError;
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    /// A table definition for DirSQL.
    #[pyclass(name = "Table", frozen)]
    struct PyTable {
        #[pyo3(get)]
        ddl: String,
        #[pyo3(get)]
        glob: String,
        extract: Py<PyAny>,
    }

    #[pymethods]
    impl PyTable {
        #[new]
        #[pyo3(signature = (*, ddl, glob, extract))]
        fn new(ddl: String, glob: String, extract: Py<PyAny>) -> Self {
            PyTable { ddl, glob, extract }
        }
    }

    /// The main DirSQL class. Creates an in-memory SQLite index over a directory.
    #[pyclass(name = "DirSQL")]
    struct PyDirSQL {
        db: Mutex<Db>,
    }

    #[pymethods]
    impl PyDirSQL {
        #[new]
        #[pyo3(signature = (root, *, tables, ignore=None))]
        fn new(
            py: Python<'_>,
            root: String,
            tables: Vec<PyRef<'_, PyTable>>,
            ignore: Option<Vec<String>>,
        ) -> PyResult<Self> {
            let db =
                Db::new().map_err(|e| PyRuntimeError::new_err(format!("DB init error: {}", e)))?;

            // Parse table names from DDLs and create tables
            let mut table_configs: Vec<(String, String, Py<PyAny>)> = Vec::new();
            for t in &tables {
                let table_name = parse_table_name(&t.ddl).ok_or_else(|| {
                    PyRuntimeError::new_err(format!(
                        "Could not parse table name from DDL: {}",
                        t.ddl
                    ))
                })?;
                db.create_table(&t.ddl)
                    .map_err(|e| PyRuntimeError::new_err(format!("DDL error: {}", e)))?;
                table_configs.push((table_name, t.glob.clone(), t.extract.clone_ref(py)));
            }

            // Build glob -> table_name mappings for the scanner
            let mappings: Vec<(&str, &str)> = table_configs
                .iter()
                .map(|(name, glob, _extract): &(String, String, Py<PyAny>)| {
                    (glob.as_str(), name.as_str())
                })
                .collect();
            let ignore_patterns: Vec<&str> = ignore
                .as_ref()
                .map(|v| v.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default();

            let matcher = TableMatcher::new(&mappings, &ignore_patterns)
                .map_err(|e| PyRuntimeError::new_err(format!("Glob error: {}", e)))?;

            // Scan directory
            let root_path = Path::new(&root);
            let files = scan_directory(root_path, &matcher);

            // Build a lookup from table_name -> extract callable
            let extract_map: HashMap<String, Py<PyAny>> = table_configs
                .iter()
                .map(|(name, _glob, extract): &(String, String, Py<PyAny>)| {
                    (name.clone(), extract.clone_ref(py))
                })
                .collect();

            // Process each file
            for (file_path, table_name) in &files {
                // Read file content
                let content = std::fs::read_to_string(file_path).map_err(|e| {
                    PyRuntimeError::new_err(format!(
                        "Failed to read {}: {}",
                        file_path.display(),
                        e
                    ))
                })?;

                // Compute relative path
                let rel_path = file_path
                    .strip_prefix(root_path)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .to_string();

                // Call extract
                let extract_fn = extract_map.get(table_name).ok_or_else(|| {
                    PyRuntimeError::new_err(format!("No extract function for table {}", table_name))
                })?;

                let result = extract_fn.call1(py, (rel_path.clone(), content))?;
                let rows: Vec<HashMap<String, Py<PyAny>>> = result.extract(py)?;

                // Insert rows
                for (row_index, py_row) in rows.iter().enumerate() {
                    let row = convert_py_row(py, py_row)?;
                    db.insert_row(table_name, &row, &rel_path, row_index)
                        .map_err(|e| PyRuntimeError::new_err(format!("Insert error: {}", e)))?;
                }
            }

            Ok(PyDirSQL { db: Mutex::new(db) })
        }

        /// Execute a SQL query and return results as a list of dicts.
        fn query(&self, py: Python<'_>, sql: &str) -> PyResult<Py<PyList>> {
            let db = self
                .db
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?;
            let rows = db
                .query(sql)
                .map_err(|e| PyRuntimeError::new_err(format!("Query error: {}", e)))?;

            let result = PyList::empty(py);
            for row in &rows {
                let dict = PyDict::new(py);
                for (key, value) in row {
                    let py_val = value_to_py(py, value);
                    dict.set_item(key, py_val)?;
                }
                result.append(dict)?;
            }
            Ok(result.unbind())
        }
    }

    /// Convert a Python dict row to a Rust HashMap<String, Value>.
    fn convert_py_row(
        py: Python<'_>,
        py_row: &HashMap<String, Py<PyAny>>,
    ) -> PyResult<HashMap<String, Value>> {
        let mut row: HashMap<String, Value> = HashMap::new();
        for (key, val) in py_row {
            let value = py_to_value(py, val)?;
            row.insert(key.clone(), value);
        }
        Ok(row)
    }

    /// Convert a Python object to a db::Value.
    fn py_to_value(py: Python<'_>, obj: &Py<PyAny>) -> PyResult<Value> {
        let bound = obj.bind(py);

        if bound.is_none() {
            return Ok(Value::Null);
        }

        // Try bool first (before int, since bool is subclass of int in Python)
        if bound.is_instance_of::<pyo3::types::PyBool>() {
            let b: bool = bound.extract()?;
            return Ok(Value::Integer(if b { 1 } else { 0 }));
        }

        // Try integer
        if let Ok(i) = bound.extract::<i64>() {
            return Ok(Value::Integer(i));
        }

        // Try float
        if let Ok(f) = bound.extract::<f64>() {
            return Ok(Value::Real(f));
        }

        // Try string
        if let Ok(s) = bound.extract::<String>() {
            return Ok(Value::Text(s));
        }

        // Try bytes
        if let Ok(b) = bound.extract::<Vec<u8>>() {
            return Ok(Value::Blob(b));
        }

        // Fall back to string representation
        let s = bound.str()?.to_string();
        Ok(Value::Text(s))
    }

    /// Convert a db::Value to a Python object.
    fn value_to_py(py: Python<'_>, value: &Value) -> Py<PyAny> {
        match value {
            Value::Null => py.None(),
            Value::Integer(i) => i.into_pyobject(py).unwrap().into_any().unbind(),
            Value::Real(f) => f.into_pyobject(py).unwrap().into_any().unbind(),
            Value::Text(s) => s.into_pyobject(py).unwrap().into_any().unbind(),
            Value::Blob(b) => b.into_pyobject(py).unwrap().unbind(),
        }
    }

    #[pymodule]
    fn dirsql(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        m.add_class::<PyTable>()?;
        m.add_class::<PyDirSQL>()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_table_name_simple() {
        assert_eq!(
            parse_table_name("CREATE TABLE comments (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_if_not_exists() {
        assert_eq!(
            parse_table_name("CREATE TABLE IF NOT EXISTS comments (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_no_space_before_paren() {
        assert_eq!(
            parse_table_name("CREATE TABLE t(id TEXT)"),
            Some("t".to_string())
        );
    }

    #[test]
    fn parse_table_name_invalid() {
        assert_eq!(parse_table_name("NOT A DDL"), None);
    }

    #[test]
    fn parse_table_name_empty_after_create_table() {
        assert_eq!(parse_table_name("CREATE TABLE "), None);
    }
}
