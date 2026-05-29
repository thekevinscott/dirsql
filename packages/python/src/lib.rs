//! PyO3 binding for `dirsql`. Intentionally thin: all orchestration lives in
//! `dirsql::DirSQL`. This layer only:
//!
//! - wraps a Python `extract` callable in a Rust closure (acquiring the GIL as
//!   needed) so it can be handed to [`dirsql::Table`]
//! - converts row dicts, values, and events between Python and Rust
//! - forwards `new` / `query` / `_start_watcher` / `_poll_events` to the
//!   corresponding `DirSQL` methods
//!
//! The Python-side async wrapper (`dirsql._async.DirSQL`) drives this binding
//! via `asyncio.to_thread`.

#[cfg(feature = "extension-module")]
mod python {
    use ::dirsql::{
        Column, ColumnType, DefaultValue, DirSQL, Expression, GeneratedColumn, GeneratedMode,
        Index, Row, RowEvent, Table, Value,
    };
    use pyo3::exceptions::{PyDeprecationWarning, PyRuntimeError, PyValueError};
    use pyo3::prelude::*;
    use pyo3::types::{PyBool, PyDict, PyList};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    // -- Public PyO3 classes ------------------------------------------------

    /// A table definition. Mirrors `dirsql::Table` but holds a Python
    /// callable for `extract`.
    ///
    /// Structured form (preferred): `Table(name=..., glob=..., columns=[...],
    /// extract=...)` plus optional table-level `primary_key` / `unique` /
    /// `indexes` / `without_rowid` / `strict_types`. Legacy form (deprecated):
    /// `Table(ddl="CREATE TABLE ...", glob=..., extract=...)`.
    #[pyclass(name = "Table", frozen)]
    struct PyTable {
        #[pyo3(get)]
        name: Option<String>,
        #[pyo3(get)]
        ddl: Option<String>,
        #[pyo3(get)]
        glob: String,
        extract: Py<PyAny>,
        #[pyo3(get)]
        strict: bool,
        columns: Vec<Column>,
        primary_key: Vec<String>,
        unique: Vec<Vec<String>>,
        indexes: Vec<Index>,
        without_rowid: bool,
        strict_types: bool,
    }

    #[pymethods]
    impl PyTable {
        #[new]
        #[pyo3(signature = (
            *,
            glob,
            extract,
            name=None,
            columns=None,
            ddl=None,
            strict=false,
            primary_key=None,
            unique=None,
            indexes=None,
            without_rowid=false,
            strict_types=false,
        ))]
        #[allow(clippy::too_many_arguments)]
        fn new(
            py: Python<'_>,
            glob: String,
            extract: Py<PyAny>,
            name: Option<String>,
            columns: Option<Bound<'_, PyAny>>,
            ddl: Option<String>,
            strict: bool,
            primary_key: Option<Vec<String>>,
            unique: Option<Vec<Vec<String>>>,
            indexes: Option<Bound<'_, PyAny>>,
            without_rowid: bool,
            strict_types: bool,
        ) -> PyResult<Self> {
            if ddl.is_some() && columns.is_some() {
                return Err(PyValueError::new_err(
                    "Table: set either `ddl` or `columns`, not both",
                ));
            }

            // Legacy `ddl=` shim: still works, but warn.
            if ddl.is_some() {
                emit_ddl_deprecation(py)?;
            }

            let parsed_columns = match &columns {
                Some(cols) => parse_columns(cols)?,
                None => Vec::new(),
            };

            if ddl.is_none() && parsed_columns.is_empty() {
                return Err(PyValueError::new_err(
                    "Table: provide structured `columns` (with a `name`) or a legacy `ddl` string",
                ));
            }
            if ddl.is_none() && name.is_none() {
                return Err(PyValueError::new_err(
                    "Table: structured tables require a `name`",
                ));
            }

            let parsed_indexes = match &indexes {
                Some(idx) => parse_indexes(idx)?,
                None => Vec::new(),
            };

            Ok(PyTable {
                name,
                ddl,
                glob,
                extract,
                strict,
                columns: parsed_columns,
                primary_key: primary_key.unwrap_or_default(),
                unique: unique.unwrap_or_default(),
                indexes: parsed_indexes,
                without_rowid,
                strict_types,
            })
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
        #[pyo3(signature = (root=None, *, tables=None, ignore=None, config=None, persist=false, persist_path=None))]
        fn new(
            py: Python<'_>,
            root: Option<String>,
            tables: Option<Vec<PyRef<'_, PyTable>>>,
            ignore: Option<Vec<String>>,
            config: Option<String>,
            persist: bool,
            persist_path: Option<PathBuf>,
        ) -> PyResult<Self> {
            let rust_tables: Vec<Table> = tables
                .as_deref()
                .map(|ts| ts.iter().map(|t| build_table(py, t)).collect())
                .unwrap_or_default();

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
                    if let Some(c) = config {
                        builder = builder.config(c);
                    }
                    if persist {
                        builder = builder.persist(true);
                    }
                    if let Some(p) = persist_path {
                        builder = builder.persist_path(p);
                    }
                    builder.build()
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
        let closure = make_extract_closure(extract_ref);
        let mut table = match &t.ddl {
            Some(ddl) => Table::try_new(ddl.clone(), t.glob.clone(), closure),
            None => Table::try_from_columns(
                t.name.clone().unwrap_or_default(),
                t.glob.clone(),
                t.columns.clone(),
                closure,
            ),
        };
        table.strict = t.strict;
        table.primary_key = t.primary_key.clone();
        table.unique = t.unique.clone();
        table.indexes = t.indexes.clone();
        table.without_rowid = t.without_rowid;
        table.strict_types = t.strict_types;
        table
    }

    /// Emit a Python `DeprecationWarning` for the legacy `ddl=` table shape.
    fn emit_ddl_deprecation(py: Python<'_>) -> PyResult<()> {
        let warnings = py.import("warnings")?;
        let category = py.get_type::<PyDeprecationWarning>();
        warnings.call_method1(
            "warn",
            (
                "Table(ddl=...) is deprecated; pass structured `columns` instead (issue #202).",
                category,
            ),
        )?;
        Ok(())
    }

    const ALLOWED_COLUMN_KEYS: &[&str] = &[
        "name",
        "type",
        "not_null",
        "primary_key",
        "unique",
        "autoincrement",
        "collate",
        "default",
        "check",
        "generated",
    ];

    fn parse_columns(value: &Bound<'_, PyAny>) -> PyResult<Vec<Column>> {
        let list = value
            .cast::<PyList>()
            .map_err(|_| PyValueError::new_err("`columns` must be a list of dicts"))?;
        let mut out = Vec::with_capacity(list.len());
        for item in list.iter() {
            out.push(parse_column(&item)?);
        }
        Ok(out)
    }

    fn parse_column(value: &Bound<'_, PyAny>) -> PyResult<Column> {
        let dict = value
            .cast::<PyDict>()
            .map_err(|_| PyValueError::new_err("each column must be a dict"))?;

        for key in dict.keys().iter() {
            let k: String = key.extract()?;
            if !ALLOWED_COLUMN_KEYS.contains(&k.as_str()) {
                return Err(PyValueError::new_err(format!("unknown column key: {k}")));
            }
        }

        let name: String = dict
            .get_item("name")?
            .ok_or_else(|| PyValueError::new_err("column is missing `name`"))?
            .extract()
            .map_err(|_| PyValueError::new_err("column `name` must be a string"))?;

        let type_str: String = dict
            .get_item("type")?
            .ok_or_else(|| PyValueError::new_err(format!("column `{name}` is missing `type`")))?
            .extract()
            .map_err(|_| {
                PyValueError::new_err(format!("column `{name}` `type` must be a string"))
            })?;
        let ty = ColumnType::parse(&type_str).ok_or_else(|| {
            PyValueError::new_err(format!(
                "column `{name}` has invalid type `{type_str}` \
                 (expected TEXT, INTEGER, REAL, BLOB, or NUMERIC)"
            ))
        })?;

        let mut col = Column::new(name.clone(), ty);
        if let Some(v) = dict.get_item("not_null")? {
            col.not_null = extract_bool(&v, "not_null", &name)?;
        }
        if let Some(v) = dict.get_item("primary_key")? {
            col.primary_key = extract_bool(&v, "primary_key", &name)?;
        }
        if let Some(v) = dict.get_item("unique")? {
            col.unique = extract_bool(&v, "unique", &name)?;
        }
        if let Some(v) = dict.get_item("autoincrement")? {
            col.autoincrement = extract_bool(&v, "autoincrement", &name)?;
        }
        if let Some(v) = dict.get_item("collate")? {
            col.collate = Some(v.extract().map_err(|_| {
                PyValueError::new_err(format!("column `{name}` `collate` must be a string"))
            })?);
        }
        if let Some(v) = dict.get_item("default")? {
            col.default = Some(parse_default(&v, &name)?);
        }
        if let Some(v) = dict.get_item("check")? {
            col.check = Some(Expression {
                sql: parse_sql_obj(&v, "check", &name)?,
            });
        }
        if let Some(v) = dict.get_item("generated")? {
            col.generated = Some(parse_generated(&v, &name)?);
        }
        Ok(col)
    }

    fn extract_bool(v: &Bound<'_, PyAny>, key: &str, col: &str) -> PyResult<bool> {
        v.extract()
            .map_err(|_| PyValueError::new_err(format!("column `{col}` `{key}` must be a bool")))
    }

    fn parse_default(v: &Bound<'_, PyAny>, col: &str) -> PyResult<DefaultValue> {
        if v.is_none() {
            return Ok(DefaultValue::Null);
        }
        if let Ok(dict) = v.cast::<PyDict>() {
            let sql: String = dict
                .get_item("sql")?
                .ok_or_else(|| {
                    PyValueError::new_err(format!(
                        "column `{col}` `default` object must have an `sql` key"
                    ))
                })?
                .extract()?;
            return Ok(DefaultValue::Sql(sql));
        }
        // bool must precede int (bool is a subclass of int in Python).
        if v.is_instance_of::<PyBool>() {
            let b: bool = v.extract()?;
            return Ok(DefaultValue::Integer(if b { 1 } else { 0 }));
        }
        if let Ok(i) = v.extract::<i64>() {
            return Ok(DefaultValue::Integer(i));
        }
        if let Ok(f) = v.extract::<f64>() {
            return Ok(DefaultValue::Real(f));
        }
        if let Ok(s) = v.extract::<String>() {
            return Ok(DefaultValue::Text(s));
        }
        if let Ok(b) = v.extract::<Vec<u8>>() {
            return Ok(DefaultValue::Blob(b));
        }
        Err(PyValueError::new_err(format!(
            "column `{col}` has an unsupported `default` value"
        )))
    }

    fn parse_sql_obj(v: &Bound<'_, PyAny>, key: &str, col: &str) -> PyResult<String> {
        let dict = v.cast::<PyDict>().map_err(|_| {
            PyValueError::new_err(format!(
                "column `{col}` `{key}` must be an object with an `sql` key"
            ))
        })?;
        dict.get_item("sql")?
            .ok_or_else(|| {
                PyValueError::new_err(format!("column `{col}` `{key}` must have an `sql` key"))
            })?
            .extract()
            .map_err(|_| {
                PyValueError::new_err(format!("column `{col}` `{key}.sql` must be a string"))
            })
    }

    fn parse_generated(v: &Bound<'_, PyAny>, col: &str) -> PyResult<GeneratedColumn> {
        let dict = v.cast::<PyDict>().map_err(|_| {
            PyValueError::new_err(format!("column `{col}` `generated` must be a dict"))
        })?;
        let sql: String = dict
            .get_item("sql")?
            .ok_or_else(|| {
                PyValueError::new_err(format!("column `{col}` `generated` must have an `sql` key"))
            })?
            .extract()?;
        let mode = match dict.get_item("mode")? {
            Some(m) => {
                let s: String = m.extract()?;
                GeneratedMode::parse(&s).ok_or_else(|| {
                    PyValueError::new_err(format!(
                        "column `{col}` `generated.mode` must be 'stored' or 'virtual'"
                    ))
                })?
            }
            None => GeneratedMode::Virtual,
        };
        Ok(GeneratedColumn { sql, mode })
    }

    fn parse_indexes(value: &Bound<'_, PyAny>) -> PyResult<Vec<Index>> {
        let list = value
            .cast::<PyList>()
            .map_err(|_| PyValueError::new_err("`indexes` must be a list of dicts"))?;
        let mut out = Vec::with_capacity(list.len());
        for item in list.iter() {
            out.push(parse_index(&item)?);
        }
        Ok(out)
    }

    fn parse_index(value: &Bound<'_, PyAny>) -> PyResult<Index> {
        let dict = value
            .cast::<PyDict>()
            .map_err(|_| PyValueError::new_err("each index must be a dict"))?;
        for key in dict.keys().iter() {
            let k: String = key.extract()?;
            if !matches!(k.as_str(), "name" | "columns" | "unique") {
                return Err(PyValueError::new_err(format!("unknown index key: {k}")));
            }
        }
        let columns: Vec<String> = dict
            .get_item("columns")?
            .ok_or_else(|| PyValueError::new_err("index is missing `columns`"))?
            .extract()
            .map_err(|_| PyValueError::new_err("index `columns` must be a list of strings"))?;
        let name = match dict.get_item("name")? {
            Some(n) if !n.is_none() => Some(n.extract()?),
            _ => None,
        };
        let unique = match dict.get_item("unique")? {
            Some(u) => u.extract()?,
            None => false,
        };
        Ok(Index {
            name,
            columns,
            unique,
        })
    }

    type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn make_extract_closure(
        extract: Py<PyAny>,
    ) -> impl Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static {
        move |path: &str| {
            Python::attach(|py| -> std::result::Result<Vec<Row>, BoxError> {
                let result = extract
                    .call1(py, (path,))
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
        // Storage-type string constants, exported for autocomplete. There is
        // no `Column` class — columns are plain dicts whose `type` is one of
        // these strings.
        m.add("TEXT", "TEXT")?;
        m.add("INTEGER", "INTEGER")?;
        m.add("REAL", "REAL")?;
        m.add("BLOB", "BLOB")?;
        m.add("NUMERIC", "NUMERIC")?;
        m.add_class::<PyTable>()?;
        m.add_class::<PyDirSQL>()?;
        m.add_class::<PyRowEvent>()?;
        Ok(())
    }
}
