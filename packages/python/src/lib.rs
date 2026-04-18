//! PyO3 binding for `dirsql`. Intentionally thin: all orchestration lives in
//! `dirsql::DirSQL`. This layer only:
//!
//! - wraps a Python `extract` callable in a Rust closure (acquiring the GIL as
//!   needed) so it can be handed to [`dirsql::Table`]
//! - converts row dicts, values, and events between Python and Rust
//! - forwards `new` / `from_config` / `query` / `_start_watcher` /
//!   `_poll_events` to the corresponding `DirSQL` methods
//!
//! The Python-side async wrapper (`dirsql._async.DirSQL`) drives this binding
//! via `asyncio.to_thread`.

#[cfg(feature = "extension-module")]
mod python {
    use ::dirsql::{DirSQL, Row, RowEvent, Table, Value};
    use pyo3::exceptions::PyRuntimeError;
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList};
    use std::collections::HashMap;
    use std::time::Duration;

    // -- Public PyO3 classes ------------------------------------------------

    /// A table definition. Mirrors `dirsql::Table` but holds a Python
    /// callable for `extract`.
    #[pyclass(name = "Table", frozen)]
    struct PyTable {
        #[pyo3(get)]
        ddl: String,
        #[pyo3(get)]
        glob: String,
        extract: Py<PyAny>,
        #[pyo3(get)]
        strict: bool,
    }

    #[pymethods]
    impl PyTable {
        #[new]
        #[pyo3(signature = (*, ddl, glob, extract, strict=false))]
        fn new(ddl: String, glob: String, extract: Py<PyAny>, strict: bool) -> Self {
            PyTable {
                ddl,
                glob,
                extract,
                strict,
            }
        }
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

    /// Synchronous binding class. `dirsql._async.DirSQL` wraps it with
    /// `asyncio.to_thread` to produce the async public API.
    #[pyclass(name = "DirSQL")]
    struct PyDirSQL {
        inner: DirSQL,
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
            let rust_tables: Vec<Table> = tables.iter().map(|t| build_table(py, t)).collect();

            let inner = py
                .detach(move || match ignore {
                    Some(ig) => DirSQL::with_ignore(root, rust_tables, ig),
                    None => DirSQL::new(root, rust_tables),
                })
                .map_err(to_py_err)?;

            Ok(Self { inner })
        }

        #[classmethod]
        fn from_config(
            _cls: &Bound<'_, pyo3::types::PyType>,
            py: Python<'_>,
            path: String,
        ) -> PyResult<Self> {
            let inner = py
                .detach(move || DirSQL::from_config_path(&path))
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

    // -- Helpers ------------------------------------------------------------

    fn build_table(py: Python<'_>, t: &PyTable) -> Table {
        let extract_ref = t.extract.clone_ref(py);
        let mut table = Table::try_new(
            t.ddl.clone(),
            t.glob.clone(),
            make_extract_closure(extract_ref),
        );
        table.strict = t.strict;
        table
    }

    type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn make_extract_closure(
        extract: Py<PyAny>,
    ) -> impl Fn(&str, &str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static
    {
        move |path: &str, content: &str| {
            Python::attach(|py| -> std::result::Result<Vec<Row>, BoxError> {
                let result = extract
                    .call1(py, (path, content))
                    .map_err(|e| -> BoxError { Box::new(ExtractError(e.to_string())) })?;
                let raw: Vec<HashMap<String, Py<PyAny>>> = result
                    .extract(py)
                    .map_err(|e: PyErr| -> BoxError { Box::new(ExtractError(e.to_string())) })?;

                let mut rows = Vec::with_capacity(raw.len());
                for r in &raw {
                    rows.push(
                        convert_py_row(py, r)
                            .map_err(|e| -> BoxError { Box::new(ExtractError(e.to_string())) })?,
                    );
                }
                Ok(rows)
            })
        }
    }

    #[derive(Debug)]
    struct ExtractError(String);
    impl std::fmt::Display for ExtractError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for ExtractError {}

    fn to_py_err<E: std::fmt::Display>(e: E) -> PyErr {
        PyRuntimeError::new_err(e.to_string())
    }

    fn row_event_to_py(py: Python<'_>, event: &RowEvent) -> PyResult<PyRowEvent> {
        Ok(match event {
            RowEvent::Insert {
                table,
                row,
                file_path,
            } => PyRowEvent {
                table: Some(table.clone()),
                action: "insert".to_string(),
                row: Some(value_row_to_py_dict(py, row)?),
                old_row: None,
                error: None,
                file_path: Some(file_path.clone()),
            },
            RowEvent::Update {
                table,
                old_row,
                new_row,
                file_path,
            } => PyRowEvent {
                table: Some(table.clone()),
                action: "update".to_string(),
                row: Some(value_row_to_py_dict(py, new_row)?),
                old_row: Some(value_row_to_py_dict(py, old_row)?),
                error: None,
                file_path: Some(file_path.clone()),
            },
            RowEvent::Delete {
                table,
                row,
                file_path,
            } => PyRowEvent {
                table: Some(table.clone()),
                action: "delete".to_string(),
                row: Some(value_row_to_py_dict(py, row)?),
                old_row: None,
                error: None,
                file_path: Some(file_path.clone()),
            },
            RowEvent::Error {
                table,
                file_path,
                error,
            } => PyRowEvent {
                table: table.clone(),
                action: "error".to_string(),
                row: None,
                old_row: None,
                error: Some(error.clone()),
                file_path: Some(file_path.to_string_lossy().to_string()),
            },
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
        if bound.is_instance_of::<pyo3::types::PyBool>() {
            let b: bool = bound.extract()?;
            return Ok(Value::Integer(if b { 1 } else { 0 }));
        }

        if let Ok(i) = bound.extract::<i64>() {
            return Ok(Value::Integer(i));
        }
        if let Ok(f) = bound.extract::<f64>() {
            return Ok(Value::Real(f));
        }
        if let Ok(s) = bound.extract::<String>() {
            return Ok(Value::Text(s));
        }
        if let Ok(b) = bound.extract::<Vec<u8>>() {
            return Ok(Value::Blob(b));
        }

        // Fall back to the Python repr.
        Ok(Value::Text(bound.str()?.to_string()))
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

    // -- Module registration ------------------------------------------------

    #[pymodule]
    #[pyo3(name = "_dirsql")]
    fn py_dirsql_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        m.add_class::<PyTable>()?;
        m.add_class::<PyDirSQL>()?;
        m.add_class::<PyRowEvent>()?;
        Ok(())
    }
}
