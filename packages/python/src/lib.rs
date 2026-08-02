//! PyO3 binding for `dirsql`. Intentionally thin: all orchestration lives in
//! `dirsql::DirSQL`. This layer only:
//!
//! - wraps a Python `on_file` callable in a Rust closure (acquiring the GIL as
//!   needed) so it can be handed to [`dirsql::Table`]
//! - converts row dicts, values, and events between Python and Rust
//! - forwards `new` / `query` / `_start_watcher` / `_poll_events` to the
//!   corresponding `DirSQL` methods
//!
//! The Python-side async wrapper (`dirsql._async.DirSQL`) drives this binding
//! via `asyncio.to_thread`.

#[cfg(feature = "extension-module")]
mod python {
    use ::dirsql::{DirSQL, Extension, Row, RowEvent, Table, Value, db::parse_table_name};
    use pyo3::exceptions::{PyOverflowError, PyRuntimeError};
    use pyo3::prelude::*;
    use pyo3::types::{PyBool, PyByteArray, PyBytes, PyDict, PyInt, PyList};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    /// A table definition. Mirrors `dirsql::Table` but holds a Python
    /// callable for `on_file`.
    #[pyclass(name = "Table", frozen)]
    struct PyTable {
        #[pyo3(get)]
        ddl: String,
        #[pyo3(get)]
        glob: String,
        #[pyo3(get)]
        on_file: Py<PyAny>,
        #[pyo3(get)]
        strict: bool,
        /// Parsed table name (from `ddl`) via `dirsql::db::parse_table_name`,
        /// or `None` if the DDL doesn't match `CREATE TABLE <name> (...)`.
        /// `None` rather than a construction error so `DirSQL.ready()` keeps
        /// surfacing malformed DDLs as the loud failure path (the core's
        /// `DirSqlError::Ddl`).
        #[pyo3(get)]
        name: Option<String>,
    }

    #[pymethods]
    impl PyTable {
        #[new]
        #[pyo3(signature = (*, ddl, glob, on_file, strict=false))]
        fn new(ddl: String, glob: String, on_file: Py<PyAny>, strict: bool) -> Self {
            let name = parse_table_name(&ddl);
            PyTable {
                ddl,
                glob,
                on_file,
                strict,
                name,
            }
        }
    }

    /// Marshals a Python `{"path": str, "entrypoint"?: str}` mapping from the
    /// `extensions=` constructor argument into a [`dirsql::Extension`]. Paths
    /// are taken verbatim; the programmatic surface does not resolve relative
    /// paths.
    #[derive(FromPyObject)]
    struct PyExtensionSpec {
        #[pyo3(item)]
        path: String,
        #[pyo3(item, default)]
        entrypoint: Option<String>,
    }

    /// A row event produced by the watch loop.
    ///
    /// `table` is `Optional[str]` because error events may occur before a
    /// file has been attributed to any table (e.g. a watch-channel failure).
    /// For insert / update / delete events it is always set.
    #[pyclass(name = "RowEvent", frozen)]
    struct PyRowEvent {
        #[pyo3(get)]
        table: Option<String>,
        #[pyo3(get)]
        action: String,
        #[pyo3(get)]
        row: Option<Py<PyDict>>,
        #[pyo3(get)]
        old_row: Option<Py<PyDict>>,
        #[pyo3(get)]
        error: Option<String>,
        #[pyo3(get)]
        file_path: Option<String>,
    }

    #[pymethods]
    impl PyRowEvent {
        fn __repr__(&self) -> String {
            format!("RowEvent(table={:?}, action={:?})", self.table, self.action)
        }
    }

    /// One file the initial scan could not index, with the hook's own error.
    ///
    /// A scan failure is not a scan *error*: the other files are indexed and
    /// the database is usable. This is how a caller learns the index is
    /// incomplete, and which files are missing from it.
    #[pyclass(name = "ScanFailure", frozen)]
    struct PyScanFailure {
        /// Path relative to the scan root.
        #[pyo3(get)]
        path: String,
        /// The hook's error, as it rendered it.
        #[pyo3(get)]
        message: String,
    }

    #[pymethods]
    impl PyScanFailure {
        fn __repr__(&self) -> String {
            format!(
                "ScanFailure(path={:?}, message={:?})",
                self.path, self.message
            )
        }
    }

    /// Synchronous binding class. `dirsql._async.DirSQL` wraps it with
    /// `asyncio.to_thread` to produce the async public API.
    #[pyclass(name = "DirSQL")]
    struct PyDirSQL {
        inner: DirSQL,
    }

    #[pymethods]
    impl PyDirSQL {
        /// `suppress_config_extensions` skips the core's own loading of the
        /// config's `[[dirsql.extension]]` entries; the SDK sets it after
        /// resolving those entries itself (package names need `importlib`,
        /// which the core lacks) and passing the resolved literal paths via
        /// `extensions`, so the entries are not loaded twice.
        #[new]
        #[pyo3(signature = (root=None, *, tables=None, ignore=None, config=None, persist=false, persist_path=None, extensions=None, suppress_config_extensions=false))]
        fn new(
            py: Python<'_>,
            root: Option<String>,
            tables: Option<Vec<PyRef<'_, PyTable>>>,
            ignore: Option<Vec<String>>,
            config: Option<Vec<String>>,
            persist: bool,
            persist_path: Option<PathBuf>,
            extensions: Option<Vec<PyExtensionSpec>>,
            suppress_config_extensions: bool,
        ) -> PyResult<Self> {
            let rust_tables: Vec<Table> = tables
                .as_deref()
                .map(|ts| ts.iter().map(|t| build_table(py, t)).collect())
                .unwrap_or_default();

            let rust_extensions: Vec<Extension> = extensions
                .unwrap_or_default()
                .into_iter()
                .map(|e| Extension {
                    path: PathBuf::from(e.path),
                    entrypoint: e.entrypoint,
                })
                .collect();

            let inner = py
                .detach(move || {
                    let mut builder = DirSQL::builder();
                    if let Some(r) = root {
                        builder = builder.root(r);
                    }
                    if !rust_tables.is_empty() {
                        builder = builder.tables(rust_tables);
                    }
                    if let Some(ig) = ignore {
                        builder = builder.ignore(ig);
                    }
                    for c in config.into_iter().flatten() {
                        builder = builder.config(c);
                    }
                    if persist {
                        builder = builder.persist(persist_path);
                    }
                    if !rust_extensions.is_empty() {
                        builder = builder.extensions(rust_extensions);
                    }
                    builder
                        .suppress_config_extensions(suppress_config_extensions)
                        .build()
                })
                .map_err(to_py_err)?;

            Ok(Self { inner })
        }

        fn query(&self, py: Python<'_>, sql: String) -> PyResult<Py<PyList>> {
            let db = self.inner.clone();
            let rows = py.detach(move || db.query(&sql)).map_err(to_py_err)?;

            let list = PyList::empty(py);
            for row in rows {
                list.append(value_row_to_py_dict(py, &row)?)?;
            }
            Ok(list.unbind())
        }

        fn scan_failures(&self) -> Vec<PyScanFailure> {
            self.inner
                .scan_failures()
                .iter()
                .map(|f| PyScanFailure {
                    path: f.path.clone(),
                    message: f.message.clone(),
                })
                .collect()
        }

        fn _start_watcher(&self, py: Python<'_>) -> PyResult<()> {
            let db = self.inner.clone();
            py.detach(move || db.start_watching()).map_err(to_py_err)
        }

        fn _poll_events(&self, py: Python<'_>, timeout_ms: u64) -> PyResult<Vec<PyRowEvent>> {
            let db = self.inner.clone();
            let events = py
                .detach(move || db.poll_events(Duration::from_millis(timeout_ms)))
                .map_err(to_py_err)?;

            events.iter().map(|e| row_event_to_py(py, e)).collect()
        }
    }

    fn build_table(py: Python<'_>, t: &PyTable) -> Table {
        let on_file_ref = t.on_file.clone_ref(py);
        let mut table = Table::try_new(
            t.ddl.clone(),
            t.glob.clone(),
            make_on_file_closure(on_file_ref),
        );
        table.strict = t.strict;
        table
    }

    type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn make_on_file_closure(
        on_file: Py<PyAny>,
    ) -> impl Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static {
        move |path: &str| {
            Python::attach(|py| -> std::result::Result<Vec<Row>, BoxError> {
                let result = on_file
                    .call1(py, (path,))
                    .map_err(|e| -> BoxError { Box::new(OnFileError(e.to_string())) })?;
                let raw: Vec<HashMap<String, Py<PyAny>>> = result
                    .extract(py)
                    .map_err(|e: PyErr| -> BoxError { Box::new(OnFileError(e.to_string())) })?;

                let mut rows = Vec::with_capacity(raw.len());
                for r in &raw {
                    rows.push(
                        convert_py_row(py, r)
                            .map_err(|e| -> BoxError { Box::new(OnFileError(e.to_string())) })?,
                    );
                }
                Ok(rows)
            })
        }
    }

    #[derive(Debug)]
    struct OnFileError(String);
    impl std::fmt::Display for OnFileError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for OnFileError {}

    fn to_py_err<E: std::fmt::Display>(e: E) -> PyErr {
        PyRuntimeError::new_err(e.to_string())
    }

    /// Pure, GIL-free intermediate for a row event. [`row_event_to_plain`]
    /// builds it from a core [`RowEvent`] (unit-testable without a Python
    /// interpreter); [`row_event_to_py`] then marshals it into the
    /// Python-facing [`PyRowEvent`] (the GIL step).
    struct PlainRowEvent {
        table: Option<String>,
        action: &'static str,
        row: Option<Row>,
        old_row: Option<Row>,
        error: Option<String>,
        file_path: String,
    }

    fn row_event_to_plain(event: &RowEvent) -> PlainRowEvent {
        match event {
            RowEvent::Insert {
                table,
                row,
                file_path,
            } => PlainRowEvent {
                table: Some(table.clone()),
                action: "insert",
                row: Some(row.clone()),
                old_row: None,
                error: None,
                file_path: file_path.clone(),
            },
            RowEvent::Update {
                table,
                old_row,
                new_row,
                file_path,
            } => PlainRowEvent {
                table: Some(table.clone()),
                action: "update",
                row: Some(new_row.clone()),
                old_row: Some(old_row.clone()),
                error: None,
                file_path: file_path.clone(),
            },
            RowEvent::Delete {
                table,
                row,
                file_path,
            } => PlainRowEvent {
                table: Some(table.clone()),
                action: "delete",
                row: Some(row.clone()),
                old_row: None,
                error: None,
                file_path: file_path.clone(),
            },
            RowEvent::Error {
                table,
                file_path,
                error,
            } => PlainRowEvent {
                table: table.clone(),
                action: "error",
                row: None,
                old_row: None,
                error: Some(error.clone()),
                file_path: file_path.to_string_lossy().to_string(),
            },
        }
    }

    fn row_event_to_py(py: Python<'_>, event: &RowEvent) -> PyResult<PyRowEvent> {
        let plain = row_event_to_plain(event);
        Ok(PyRowEvent {
            table: plain.table,
            action: plain.action.to_string(),
            row: plain
                .row
                .as_ref()
                .map(|r| value_row_to_py_dict(py, r))
                .transpose()?,
            old_row: plain
                .old_row
                .as_ref()
                .map(|r| value_row_to_py_dict(py, r))
                .transpose()?,
            error: plain.error,
            file_path: Some(plain.file_path),
        })
    }

    fn value_row_to_py_dict(py: Python<'_>, row: &Row) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in row {
            dict.set_item(key, value_to_py(py, value))?;
        }
        Ok(dict.unbind())
    }

    fn convert_py_row(
        py: Python<'_>,
        py_row: &HashMap<String, Py<PyAny>>,
    ) -> PyResult<HashMap<String, Value>> {
        let mut row = HashMap::new();
        for (key, val) in py_row {
            row.insert(key.clone(), py_to_value(py, val)?);
        }
        Ok(row)
    }

    fn py_to_value(py: Python<'_>, obj: &Py<PyAny>) -> PyResult<Value> {
        let bound = obj.bind(py);

        if bound.is_none() {
            return Ok(Value::Null);
        }

        // bool must precede int (bool is a subclass of int in Python).
        if bound.is_instance_of::<PyBool>() {
            let b: bool = bound.extract()?;
            return Ok(Value::Integer(if b { 1 } else { 0 }));
        }

        // A Python int must round-trip losslessly or fail loudly. Falling
        // through to f64/str would silently corrupt an out-of-i64 value
        // (lossy Real, or a TEXT repr).
        if bound.is_instance_of::<PyInt>() {
            return match bound.extract::<i64>() {
                Ok(i) => Ok(Value::Integer(i)),
                Err(_) => Err(PyOverflowError::new_err(int_overflow_message(
                    &bound.str()?.to_string(),
                ))),
            };
        }

        if let Ok(f) = bound.extract::<f64>() {
            return Ok(Value::Real(f));
        }
        if let Ok(s) = bound.extract::<String>() {
            return Ok(Value::Text(s));
        }
        // Only genuine binary types map to BLOB. A list/tuple of ints must
        // NOT be probed as bytes (that turned `[1,2,3]` into a BLOB by
        // magnitude); it falls through to the repr like any other sequence.
        if bound.is_instance_of::<PyBytes>() || bound.is_instance_of::<PyByteArray>() {
            let b: Vec<u8> = bound.extract()?;
            return Ok(Value::Blob(b));
        }

        // Fall back to the Python repr.
        Ok(Value::Text(bound.str()?.to_string()))
    }

    /// The range-error message for a Python int that does not fit `i64`.
    fn int_overflow_message(repr: &str) -> String {
        format!("integer {repr} exceeds the 64-bit signed range dirsql can store")
    }

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
    #[pyo3(name = "_dirsql")]
    fn py_dirsql_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        m.add_class::<PyTable>()?;
        m.add_class::<PyDirSQL>()?;
        m.add_class::<PyRowEvent>()?;
        m.add_class::<PyScanFailure>()?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn one_row() -> HashMap<String, Value> {
            HashMap::from([("k".to_string(), Value::Integer(7))])
        }

        #[test]
        fn plain_insert_maps_row_and_action() {
            let p = row_event_to_plain(&RowEvent::Insert {
                table: "t".into(),
                row: one_row(),
                file_path: "/f".into(),
            });
            assert_eq!(p.action, "insert");
            assert_eq!(p.table.as_deref(), Some("t"));
            assert_eq!(
                p.row.as_ref().and_then(|r| r.get("k")),
                Some(&Value::Integer(7))
            );
            assert!(p.old_row.is_none());
            assert!(p.error.is_none());
            assert_eq!(p.file_path, "/f");
        }

        #[test]
        fn plain_update_carries_old_and_new() {
            let mut new = one_row();
            new.insert("k".to_string(), Value::Integer(9));
            let p = row_event_to_plain(&RowEvent::Update {
                table: "t".into(),
                old_row: one_row(),
                new_row: new,
                file_path: "/f".into(),
            });
            assert_eq!(p.action, "update");
            assert_eq!(
                p.row.as_ref().and_then(|r| r.get("k")),
                Some(&Value::Integer(9))
            );
            assert_eq!(
                p.old_row.as_ref().and_then(|r| r.get("k")),
                Some(&Value::Integer(7))
            );
        }

        #[test]
        fn plain_delete_has_row_no_old() {
            let p = row_event_to_plain(&RowEvent::Delete {
                table: "t".into(),
                row: one_row(),
                file_path: "/f".into(),
            });
            assert_eq!(p.action, "delete");
            assert!(p.row.is_some());
            assert!(p.old_row.is_none());
        }

        #[test]
        fn plain_error_has_no_row_and_optional_table() {
            let p = row_event_to_plain(&RowEvent::Error {
                table: None,
                file_path: std::path::PathBuf::from("/f"),
                error: "boom".into(),
            });
            assert_eq!(p.action, "error");
            assert!(p.table.is_none());
            assert!(p.row.is_none());
            assert!(p.old_row.is_none());
            assert_eq!(p.error.as_deref(), Some("boom"));
            assert_eq!(p.file_path, "/f");
        }

        #[test]
        fn on_file_error_displays_inner() {
            assert_eq!(OnFileError("bad".to_string()).to_string(), "bad");
        }

        #[test]
        fn int_overflow_message_names_the_value() {
            let m = int_overflow_message("9223372036854775808");
            assert!(m.contains("9223372036854775808"));
            assert!(m.contains("exceeds"));
        }
    }
}
