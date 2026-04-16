#[cfg(feature = "extension-module")]
mod python {
    use dirsql::config;
    use dirsql::db::{Db, Value, parse_table_name};
    use dirsql::differ;
    use dirsql::matcher::TableMatcher;
    use dirsql::parser::{self, ColumnSource, Format};
    use dirsql::scanner::scan_directory;
    use dirsql::watcher::{FileEvent, Watcher};
    use pyo3::exceptions::PyRuntimeError;
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList};
    use regex::Regex;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    /// A table definition for DirSQL.
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

    /// A row event emitted by the watch stream.
    #[pyclass(name = "RowEvent", frozen)]
    struct PyRowEvent {
        #[pyo3(get)]
        table: String,
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

    /// Rows extracted from a file: (table_name, rows).
    type FileRows = (String, Vec<HashMap<String, Value>>);

    /// How rows are extracted from a file.
    enum ExtractMode {
        /// Python callable: fn(rel_path, content) -> list[dict]
        Python(Py<PyAny>),
        /// Built-in parser from config file.
        BuiltIn {
            format: Format,
            each: Option<String>,
            columns: HashMap<String, ColumnSource>,
            capture_names: Vec<String>,
        },
    }

    /// Internal table config stored after init for use during watch.
    struct TableConfig {
        name: String,
        glob: String,
        extract: ExtractMode,
        strict: bool,
    }

    /// Extract `{name}` capture placeholders from a glob pattern.
    fn extract_capture_names(glob: &str) -> Vec<String> {
        let re = Regex::new(r"\{([a-zA-Z_][a-zA-Z0-9_]*)\}").unwrap();
        re.captures_iter(glob).map(|c| c[1].to_string()).collect()
    }

    /// The main DirSQL class. Creates an in-memory SQLite index over a directory.
    #[pyclass(name = "DirSQL")]
    struct PyDirSQL {
        db: Mutex<Db>,
        root: PathBuf,
        table_configs: Vec<TableConfig>,
        ignore_patterns: Vec<String>,
        /// Tracks rows per file for diffing: file_rel_path -> (table_name, rows)
        file_rows: Mutex<HashMap<String, FileRows>>,
        watcher: Mutex<Option<Watcher>>,
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
            let mut parsed_configs: Vec<(String, String, Py<PyAny>, bool)> = Vec::new();
            for t in &tables {
                let table_name = parse_table_name(&t.ddl).ok_or_else(|| {
                    PyRuntimeError::new_err(format!(
                        "Could not parse table name from DDL: {}",
                        t.ddl
                    ))
                })?;
                db.create_table(&t.ddl)
                    .map_err(|e| PyRuntimeError::new_err(format!("DDL error: {}", e)))?;
                parsed_configs.push((
                    table_name,
                    t.glob.clone(),
                    t.extract.clone_ref(py),
                    t.strict,
                ));
            }

            // Build glob -> table_name mappings for the scanner
            let mappings: Vec<(&str, &str)> = parsed_configs
                .iter()
                .map(|(name, glob, _extract, _strict)| (glob.as_str(), name.as_str()))
                .collect();
            let ignore_strs: Vec<&str> = ignore
                .as_ref()
                .map(|v| v.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default();

            let matcher = TableMatcher::new(&mappings, &ignore_strs)
                .map_err(|e| PyRuntimeError::new_err(format!("Glob error: {}", e)))?;

            // Scan directory
            let root_path = Path::new(&root);
            let files = scan_directory(root_path, &matcher);

            // Build a lookup from table_name -> extract callable
            let extract_map: HashMap<String, Py<PyAny>> = parsed_configs
                .iter()
                .map(|(name, _glob, extract, _strict)| (name.clone(), extract.clone_ref(py)))
                .collect();

            // Build a lookup from table_name -> strict flag
            let strict_map: HashMap<String, bool> = parsed_configs
                .iter()
                .map(|(name, _glob, _extract, strict)| (name.clone(), *strict))
                .collect();

            // Track rows per file for later diffing
            let mut file_rows: HashMap<String, (String, Vec<HashMap<String, Value>>)> =
                HashMap::new();

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

                // Convert and insert rows, tracking them
                let strict = *strict_map.get(table_name).unwrap_or(&false);
                let mut value_rows: Vec<HashMap<String, Value>> = Vec::new();
                for (row_index, py_row) in rows.iter().enumerate() {
                    let raw_row = convert_py_row(py, py_row)?;
                    let row = db
                        .normalize_row(table_name, &raw_row, strict)
                        .map_err(|e| PyRuntimeError::new_err(format!("Schema error: {}", e)))?;
                    db.insert_row(table_name, &row, &rel_path, row_index)
                        .map_err(|e| PyRuntimeError::new_err(format!("Insert error: {}", e)))?;
                    value_rows.push(row);
                }
                file_rows.insert(rel_path, (table_name.clone(), value_rows));
            }

            // Store table configs for watch use
            let stored_configs: Vec<TableConfig> = parsed_configs
                .into_iter()
                .map(|(name, glob, extract, strict)| TableConfig {
                    name,
                    glob,
                    extract: ExtractMode::Python(extract),
                    strict,
                })
                .collect();

            Ok(PyDirSQL {
                db: Mutex::new(db),
                root: PathBuf::from(&root),
                table_configs: stored_configs,
                ignore_patterns: ignore.unwrap_or_default(),
                file_rows: Mutex::new(file_rows),
                watcher: Mutex::new(None),
            })
        }

        /// Create a DirSQL instance from a .dirsql.toml config file.
        #[classmethod]
        fn from_config(_cls: &Bound<'_, pyo3::types::PyType>, path: String) -> PyResult<Self> {
            let config_path = Path::new(&path);
            let config = config::load_config(config_path)
                .map_err(|e| PyRuntimeError::new_err(format!("Config error: {}", e)))?;

            // Derive root directory from config file path
            let root = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();

            let db =
                Db::new().map_err(|e| PyRuntimeError::new_err(format!("DB init error: {}", e)))?;

            let mut table_configs: Vec<TableConfig> = Vec::new();
            let mut parsed_names: Vec<(String, String)> = Vec::new(); // (table_name, glob)

            for tc in &config.tables {
                let table_name = parse_table_name(&tc.ddl).ok_or_else(|| {
                    PyRuntimeError::new_err(format!(
                        "Could not parse table name from DDL: {}",
                        tc.ddl
                    ))
                })?;
                db.create_table(&tc.ddl)
                    .map_err(|e| PyRuntimeError::new_err(format!("DDL error: {}", e)))?;

                let format = tc.format.ok_or_else(|| {
                    PyRuntimeError::new_err(format!(
                        "Unsupported format: cannot infer format for glob '{}'. \
                         Specify format explicitly in the config.",
                        tc.glob
                    ))
                })?;

                let capture_names = extract_capture_names(&tc.glob);
                let columns: HashMap<String, ColumnSource> = tc
                    .columns
                    .as_ref()
                    .map(|cols| {
                        cols.iter()
                            .map(|(k, v)| (k.clone(), ColumnSource::parse(v, &capture_names)))
                            .collect()
                    })
                    .unwrap_or_default();

                parsed_names.push((table_name.clone(), tc.glob.clone()));
                table_configs.push(TableConfig {
                    name: table_name,
                    glob: tc.glob.clone(),
                    extract: ExtractMode::BuiltIn {
                        format,
                        each: tc.each.clone(),
                        columns,
                        capture_names,
                    },
                    strict: tc.strict.unwrap_or(false),
                });
            }

            // Build glob -> table_name mappings for the scanner
            let mappings: Vec<(&str, &str)> = parsed_names
                .iter()
                .map(|(name, glob)| (glob.as_str(), name.as_str()))
                .collect();
            let ignore_strs: Vec<&str> = config.ignore.iter().map(|s| s.as_str()).collect();

            let matcher = TableMatcher::new(&mappings, &ignore_strs)
                .map_err(|e| PyRuntimeError::new_err(format!("Glob error: {}", e)))?;

            // Scan directory
            let files = scan_directory(&root, &matcher);

            // Build lookups for processing
            let config_map: HashMap<&str, &TableConfig> = table_configs
                .iter()
                .map(|tc| (tc.name.as_str(), tc))
                .collect();

            let mut file_rows: HashMap<String, (String, Vec<HashMap<String, Value>>)> =
                HashMap::new();

            for (file_path, table_name) in &files {
                let content = std::fs::read_to_string(file_path).map_err(|e| {
                    PyRuntimeError::new_err(format!(
                        "Failed to read {}: {}",
                        file_path.display(),
                        e
                    ))
                })?;

                let rel_path = file_path
                    .strip_prefix(&root)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .to_string();

                let tc = config_map.get(table_name.as_str()).ok_or_else(|| {
                    PyRuntimeError::new_err(format!("No config for table {}", table_name))
                })?;

                let rows = match &tc.extract {
                    ExtractMode::BuiltIn {
                        format,
                        each,
                        columns,
                        capture_names: _,
                    } => {
                        let mut parsed = parser::parse_file(*format, &content, each.as_deref())
                            .map_err(|e| PyRuntimeError::new_err(format!("Parse error: {}", e)))?;

                        // Get path captures
                        let rel = Path::new(&rel_path);
                        let captures = matcher
                            .match_file_with_captures(rel)
                            .map(|m| m.captures)
                            .unwrap_or_default();

                        // Apply column mapping and path captures
                        parsed = parser::apply_columns(&parsed, columns, &captures);

                        parsed
                    }
                    ExtractMode::Python(_) => {
                        unreachable!("from_config never uses Python extract")
                    }
                };

                let strict = tc.strict;
                let mut value_rows: Vec<HashMap<String, Value>> = Vec::new();
                for (row_index, raw_row) in rows.iter().enumerate() {
                    let row = db
                        .normalize_row(table_name, raw_row, strict)
                        .map_err(|e| PyRuntimeError::new_err(format!("Schema error: {}", e)))?;
                    db.insert_row(table_name, &row, &rel_path, row_index)
                        .map_err(|e| PyRuntimeError::new_err(format!("Insert error: {}", e)))?;
                    value_rows.push(row);
                }
                file_rows.insert(rel_path, (table_name.clone(), value_rows));
            }

            Ok(PyDirSQL {
                db: Mutex::new(db),
                root,
                table_configs,
                ignore_patterns: config.ignore,
                file_rows: Mutex::new(file_rows),
                watcher: Mutex::new(None),
            })
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

        /// Start the file watcher. Must be called before poll_events.
        fn _start_watcher(&self) -> PyResult<()> {
            let watcher = Watcher::new(&self.root)
                .map_err(|e| PyRuntimeError::new_err(format!("Watcher error: {}", e)))?;
            let mut guard = self
                .watcher
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?;
            *guard = Some(watcher);
            Ok(())
        }

        /// Poll for file events with a timeout (in milliseconds).
        /// Returns a list of PyRowEvent objects, possibly empty if no events within timeout.
        fn _poll_events(&self, py: Python<'_>, timeout_ms: u64) -> PyResult<Vec<PyRowEvent>> {
            // Collect raw file events from watcher
            let file_events = {
                let guard = self
                    .watcher
                    .lock()
                    .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?;
                let watcher = guard.as_ref().ok_or_else(|| {
                    PyRuntimeError::new_err("Watcher not started. Call _start_watcher first.")
                })?;

                // Wait for at least one event, then drain remaining
                let mut events = Vec::new();
                if let Some(event) = watcher.recv_timeout(Duration::from_millis(timeout_ms)) {
                    events.push(event);
                    // Drain any additional pending events
                    events.extend(watcher.try_recv_all());
                }
                events
            };

            if file_events.is_empty() {
                return Ok(Vec::new());
            }

            // Build matcher for determining which table a file belongs to
            let mappings: Vec<(&str, &str)> = self
                .table_configs
                .iter()
                .map(|tc| (tc.glob.as_str(), tc.name.as_str()))
                .collect();
            let ignore_strs: Vec<&str> = self.ignore_patterns.iter().map(|s| s.as_str()).collect();
            let matcher = TableMatcher::new(&mappings, &ignore_strs)
                .map_err(|e| PyRuntimeError::new_err(format!("Glob error: {}", e)))?;

            // Build config lookup
            let config_map: HashMap<&str, &TableConfig> = self
                .table_configs
                .iter()
                .map(|tc| (tc.name.as_str(), tc))
                .collect();

            let mut result_events = Vec::new();

            for file_event in file_events {
                let abs_path = match &file_event {
                    FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => p,
                };

                // Match against relative path so globs like "comments/**/*.jsonl" work
                let rel_for_match = abs_path.strip_prefix(&self.root).unwrap_or(abs_path);

                // Skip files that don't match any table or are ignored
                if matcher.is_ignored(rel_for_match) {
                    continue;
                }
                let table_name = match matcher.match_file(rel_for_match) {
                    Some(name) => name.to_string(),
                    None => continue,
                };

                let rel_path = abs_path
                    .strip_prefix(&self.root)
                    .unwrap_or(abs_path)
                    .to_string_lossy()
                    .to_string();

                match file_event {
                    FileEvent::Deleted(_) => {
                        // Get old rows and produce delete events
                        let mut file_rows = self
                            .file_rows
                            .lock()
                            .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?;
                        let old_entry = file_rows.remove(&rel_path);
                        let old_rows = old_entry.as_ref().map(|(_, rows)| rows.as_slice());

                        let row_events = differ::diff(&table_name, old_rows, None, &rel_path);

                        // Update DB
                        let db = self
                            .db
                            .lock()
                            .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?;
                        db.delete_rows_by_file(&table_name, &rel_path)
                            .map_err(|e| PyRuntimeError::new_err(format!("DB error: {}", e)))?;

                        for re in row_events {
                            result_events.push(row_event_to_py(py, &re, &rel_path)?);
                        }
                    }
                    FileEvent::Created(_) | FileEvent::Modified(_) => {
                        // Read the file
                        let content = match std::fs::read_to_string(abs_path) {
                            Ok(c) => c,
                            Err(e) => {
                                // File might have been deleted between event and read
                                if e.kind() == std::io::ErrorKind::NotFound {
                                    continue;
                                }
                                return Err(PyRuntimeError::new_err(format!(
                                    "Failed to read {}: {}",
                                    abs_path.display(),
                                    e
                                )));
                            }
                        };

                        let tc = match config_map.get(table_name.as_str()) {
                            Some(tc) => tc,
                            None => continue,
                        };

                        let new_rows = match &tc.extract {
                            ExtractMode::Python(extract_fn) => {
                                let extract_result =
                                    extract_fn.call1(py, (rel_path.clone(), content));
                                match extract_result {
                                    Ok(result) => {
                                        let py_rows: Result<Vec<HashMap<String, Py<PyAny>>>, _> =
                                            result.extract(py);
                                        match py_rows {
                                            Ok(rows) => {
                                                let db = self.db.lock().map_err(|e| {
                                                    PyRuntimeError::new_err(format!(
                                                        "Lock error: {}",
                                                        e
                                                    ))
                                                })?;
                                                let mut value_rows = Vec::new();
                                                for py_row in &rows {
                                                    match convert_py_row(py, py_row).and_then(
                                                        |raw| {
                                                            db.normalize_row(
                                                                &table_name,
                                                                &raw,
                                                                tc.strict,
                                                            )
                                                            .map_err(|e| {
                                                                PyRuntimeError::new_err(format!(
                                                                    "Schema error: {}",
                                                                    e
                                                                ))
                                                            })
                                                        },
                                                    ) {
                                                        Ok(r) => value_rows.push(r),
                                                        Err(e) => {
                                                            result_events.push(PyRowEvent {
                                                                table: table_name.clone(),
                                                                action: "error".to_string(),
                                                                row: None,
                                                                old_row: None,
                                                                error: Some(format!(
                                                                    "Row conversion error: {}",
                                                                    e
                                                                )),
                                                                file_path: Some(rel_path.clone()),
                                                            });
                                                            continue;
                                                        }
                                                    }
                                                }
                                                drop(db);
                                                value_rows
                                            }
                                            Err(e) => {
                                                result_events.push(PyRowEvent {
                                                    table: table_name.clone(),
                                                    action: "error".to_string(),
                                                    row: None,
                                                    old_row: None,
                                                    error: Some(format!(
                                                        "Extract result error: {}",
                                                        e
                                                    )),
                                                    file_path: Some(rel_path.clone()),
                                                });
                                                continue;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        result_events.push(PyRowEvent {
                                            table: table_name.clone(),
                                            action: "error".to_string(),
                                            row: None,
                                            old_row: None,
                                            error: Some(format!("Extract error: {}", e)),
                                            file_path: Some(rel_path.clone()),
                                        });
                                        continue;
                                    }
                                }
                            }
                            ExtractMode::BuiltIn {
                                format,
                                each,
                                columns,
                                capture_names: _,
                            } => match parser::parse_file(*format, &content, each.as_deref()) {
                                Ok(mut parsed) => {
                                    let rel = Path::new(&rel_path);
                                    let captures = matcher
                                        .match_file_with_captures(rel)
                                        .map(|m| m.captures)
                                        .unwrap_or_default();
                                    parsed = parser::apply_columns(&parsed, columns, &captures);

                                    let db = self.db.lock().map_err(|e| {
                                        PyRuntimeError::new_err(format!("Lock error: {}", e))
                                    })?;
                                    let mut value_rows = Vec::new();
                                    for raw_row in &parsed {
                                        match db.normalize_row(&table_name, raw_row, tc.strict) {
                                            Ok(r) => value_rows.push(r),
                                            Err(e) => {
                                                result_events.push(PyRowEvent {
                                                    table: table_name.clone(),
                                                    action: "error".to_string(),
                                                    row: None,
                                                    old_row: None,
                                                    error: Some(format!("Schema error: {}", e)),
                                                    file_path: Some(rel_path.clone()),
                                                });
                                                continue;
                                            }
                                        }
                                    }
                                    drop(db);
                                    value_rows
                                }
                                Err(e) => {
                                    result_events.push(PyRowEvent {
                                        table: table_name.clone(),
                                        action: "error".to_string(),
                                        row: None,
                                        old_row: None,
                                        error: Some(format!("Parse error: {}", e)),
                                        file_path: Some(rel_path.clone()),
                                    });
                                    continue;
                                }
                            },
                        };

                        // Get old rows for diffing
                        let mut file_rows = self
                            .file_rows
                            .lock()
                            .map_err(|e| PyRuntimeError::new_err(format!("Lock error: {}", e)))?;
                        let old_entry = file_rows.get(&rel_path);
                        let old_rows = old_entry.map(|(_, rows)| rows.as_slice());

                        let row_events =
                            differ::diff(&table_name, old_rows, Some(&new_rows), &rel_path);

                        // Update DB: delete old rows, insert new ones
                        {
                            let db = self.db.lock().map_err(|e| {
                                PyRuntimeError::new_err(format!("Lock error: {}", e))
                            })?;
                            db.delete_rows_by_file(&table_name, &rel_path)
                                .map_err(|e| PyRuntimeError::new_err(format!("DB error: {}", e)))?;
                            for (row_index, row) in new_rows.iter().enumerate() {
                                db.insert_row(&table_name, row, &rel_path, row_index)
                                    .map_err(|e| {
                                        PyRuntimeError::new_err(format!("Insert error: {}", e))
                                    })?;
                            }
                        }

                        // Update file_rows tracking
                        file_rows.insert(rel_path.clone(), (table_name.clone(), new_rows));

                        for re in row_events {
                            result_events.push(row_event_to_py(py, &re, &rel_path)?);
                        }
                    }
                }
            }

            Ok(result_events)
        }
    }

    /// Convert a differ::RowEvent into a PyRowEvent.
    fn row_event_to_py(
        py: Python<'_>,
        event: &differ::RowEvent,
        _rel_path: &str,
    ) -> PyResult<PyRowEvent> {
        match event {
            differ::RowEvent::Insert {
                table,
                row,
                file_path,
            } => {
                let dict = value_row_to_py_dict(py, row)?;
                Ok(PyRowEvent {
                    table: table.clone(),
                    action: "insert".to_string(),
                    row: Some(dict),
                    old_row: None,
                    error: None,
                    file_path: Some(file_path.clone()),
                })
            }
            differ::RowEvent::Update {
                table,
                old_row,
                new_row,
                file_path,
            } => {
                let new_dict = value_row_to_py_dict(py, new_row)?;
                let old_dict = value_row_to_py_dict(py, old_row)?;
                Ok(PyRowEvent {
                    table: table.clone(),
                    action: "update".to_string(),
                    row: Some(new_dict),
                    old_row: Some(old_dict),
                    error: None,
                    file_path: Some(file_path.clone()),
                })
            }
            differ::RowEvent::Delete {
                table,
                row,
                file_path,
            } => {
                let dict = value_row_to_py_dict(py, row)?;
                Ok(PyRowEvent {
                    table: table.clone(),
                    action: "delete".to_string(),
                    row: Some(dict),
                    old_row: None,
                    error: None,
                    file_path: Some(file_path.clone()),
                })
            }
            differ::RowEvent::Error { file_path, error } => Ok(PyRowEvent {
                table: String::new(),
                action: "error".to_string(),
                row: None,
                old_row: None,
                error: Some(error.clone()),
                file_path: Some(file_path.to_string_lossy().to_string()),
            }),
        }
    }

    /// Convert a HashMap<String, Value> to a Python dict.
    fn value_row_to_py_dict(py: Python<'_>, row: &HashMap<String, Value>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in row {
            let py_val = value_to_py(py, value);
            dict.set_item(key, py_val)?;
        }
        Ok(dict.unbind())
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
    #[pyo3(name = "_dirsql")]
    fn dirsql(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add("__version__", env!("CARGO_PKG_VERSION"))?;
        m.add_class::<PyTable>()?;
        m.add_class::<PyDirSQL>()?;
        m.add_class::<PyRowEvent>()?;
        Ok(())
    }
}
