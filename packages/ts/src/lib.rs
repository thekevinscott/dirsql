use dirsql_core::db::{parse_table_name, Db, Value};
use dirsql_core::differ;
use dirsql_core::matcher::TableMatcher;
use dirsql_core::scanner::scan_directory;
use dirsql_core::watcher::{FileEvent, Watcher};
use napi::bindgen_prelude::*;
use napi::JsObject;
use napi_derive::napi;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// A row event emitted by the watch stream.
#[napi(object)]
pub struct RowEvent {
    pub table: String,
    pub action: String,
    pub row: Option<HashMap<String, serde_json::Value>>,
    pub old_row: Option<HashMap<String, serde_json::Value>>,
    pub error: Option<String>,
    pub file_path: Option<String>,
}

/// Wrapper around napi::Ref that is Send.
///
/// SAFETY: This is safe because:
/// - All #[napi] methods run on the JS main thread
/// - The Ref is only accessed from within #[napi] methods
/// - We never send the Ref to another thread for actual use
struct SendableRef(napi::Ref<()>);

// SAFETY: See above. The Ref is only ever accessed on the JS main thread.
unsafe impl Send for SendableRef {}
// SAFETY: We only access via &self on the JS thread. Mutex<Db> handles interior mutability.
unsafe impl Sync for SendableRef {}

/// Internal table config stored after init for use during watch.
struct TableConfig {
    name: String,
    glob: String,
    extract_ref: SendableRef,
    strict: bool,
}

/// The main DirSQL class. Creates an in-memory SQLite index over a directory.
#[napi]
pub struct DirSQL {
    db: Mutex<Db>,
    root: PathBuf,
    table_configs: Vec<TableConfig>,
    ignore_patterns: Vec<String>,
    /// Tracks rows per file for diffing: file_rel_path -> (table_name, rows)
    file_rows: Mutex<HashMap<String, (String, Vec<HashMap<String, Value>>)>>,
    watcher: Mutex<Option<Watcher>>,
}

/// Convert a dirsql_core Value to serde_json::Value.
fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Integer(i) => serde_json::Value::Number((*i).into()),
        Value::Real(f) => serde_json::json!(*f),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Blob(b) => {
            use std::fmt::Write;
            let mut hex = String::with_capacity(b.len() * 2);
            for byte in b {
                write!(hex, "{:02x}", byte).unwrap();
            }
            serde_json::Value::String(hex)
        }
    }
}

/// Convert a HashMap<String, Value> to a serde_json map for returning to JS.
fn value_row_to_json(row: &HashMap<String, Value>) -> HashMap<String, serde_json::Value> {
    row.iter()
        .map(|(k, v)| (k.clone(), value_to_json(v)))
        .collect()
}

/// Call the extract function via its Ref and parse the result.
fn call_extract(
    env: &Env,
    extract_ref: &SendableRef,
    rel_path: &str,
    content: &str,
) -> Result<Vec<HashMap<String, Value>>> {
    let extract_fn: JsFunction = env.get_reference_value(&extract_ref.0)?;
    let js_path = env.create_string(rel_path)?.into_unknown();
    let js_content = env.create_string(content)?.into_unknown();
    let result: JsObject = extract_fn.call(None, &[js_path, js_content])?.try_into()?;

    // Convert JS array of objects to Vec<HashMap<String, Value>>
    let len: u32 = result.get_array_length()?;
    let mut rows = Vec::with_capacity(len as usize);

    for i in 0..len {
        let item_unknown: napi::JsUnknown = result.get_element(i)?;
        let item: JsObject = item_unknown.try_into()?;
        let keys = JsObject::keys(&item)?;
        let mut row = HashMap::new();

        for key in &keys {
            let val: napi::JsUnknown = item.get_named_property(key)?;
            let value = js_unknown_to_value(env, val)?;
            row.insert(key.clone(), value);
        }
        rows.push(row);
    }

    Ok(rows)
}

/// Convert a JsUnknown to a dirsql_core Value.
fn js_unknown_to_value(_env: &Env, val: napi::JsUnknown) -> Result<Value> {
    match val.get_type()? {
        napi::ValueType::Null | napi::ValueType::Undefined => Ok(Value::Null),
        napi::ValueType::Boolean => {
            let b: bool = val.coerce_to_bool()?.get_value()?;
            Ok(Value::Integer(if b { 1 } else { 0 }))
        }
        napi::ValueType::Number => {
            let n: f64 = val.coerce_to_number()?.get_double()?;
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                Ok(Value::Integer(n as i64))
            } else {
                Ok(Value::Real(n))
            }
        }
        napi::ValueType::String => {
            let s: String = unsafe { val.cast::<napi::JsString>() }
                .into_utf8()?
                .as_str()?
                .to_string();
            Ok(Value::Text(s))
        }
        _ => {
            let s: String = val.coerce_to_string()?.into_utf8()?.as_str()?.to_string();
            Ok(Value::Text(s))
        }
    }
}

#[napi]
impl DirSQL {
    /// Create a new DirSQL instance.
    ///
    /// @param root - Root directory path to index
    /// @param tables - Array of table definition objects { ddl, glob, extract, strict? }
    /// @param ignore - Optional array of glob patterns to ignore
    #[napi(constructor)]
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        env: Env,
        root: String,
        tables: napi::JsObject,
        ignore: Option<Vec<String>>,
    ) -> Result<Self> {
        let db = Db::new()
            .map_err(|e| Error::new(Status::GenericFailure, format!("DB init error: {}", e)))?;

        let root_path = PathBuf::from(&root);
        let mut table_configs: Vec<TableConfig> = Vec::new();

        let tables_len: u32 = tables.get_array_length()?;
        for i in 0..tables_len {
            let table_unknown: napi::JsUnknown = tables.get_element(i)?;
            let table_obj: JsObject = table_unknown.try_into()?;
            let ddl: String = table_obj.get_named_property("ddl")?;
            let glob: String = table_obj.get_named_property("glob")?;
            let extract_fn: JsFunction = table_obj.get_named_property("extract")?;
            let strict: bool = table_obj
                .get_named_property::<bool>("strict")
                .unwrap_or(false);

            let table_name = parse_table_name(&ddl).ok_or_else(|| {
                Error::new(
                    Status::GenericFailure,
                    format!("Could not parse table name from DDL: {}", ddl),
                )
            })?;
            db.create_table(&ddl)
                .map_err(|e| Error::new(Status::GenericFailure, format!("DDL error: {}", e)))?;

            let extract_ref = env.create_reference(extract_fn)?;

            table_configs.push(TableConfig {
                name: table_name,
                glob,
                extract_ref: SendableRef(extract_ref),
                strict,
            });
        }

        // Build glob -> table_name mappings for the scanner
        let mappings: Vec<(&str, &str)> = table_configs
            .iter()
            .map(|tc| (tc.glob.as_str(), tc.name.as_str()))
            .collect();
        let ignore_strs: Vec<&str> = ignore
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        let matcher = TableMatcher::new(&mappings, &ignore_strs)
            .map_err(|e| Error::new(Status::GenericFailure, format!("Glob error: {}", e)))?;

        // Scan directory
        let files = scan_directory(&root_path, &matcher);

        // Track rows per file
        let mut file_rows: HashMap<String, (String, Vec<HashMap<String, Value>>)> = HashMap::new();

        // Process each file
        for (file_path, table_name) in &files {
            let content = std::fs::read_to_string(file_path).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("Failed to read {}: {}", file_path.display(), e),
                )
            })?;

            let rel_path = file_path
                .strip_prefix(&root_path)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string();

            let tc = table_configs
                .iter()
                .find(|tc| &tc.name == table_name)
                .ok_or_else(|| {
                    Error::new(
                        Status::GenericFailure,
                        format!("No extract function for table {}", table_name),
                    )
                })?;

            let rows = call_extract(&env, &tc.extract_ref, &rel_path, &content)?;

            let mut value_rows: Vec<HashMap<String, Value>> = Vec::new();
            for (row_index, row) in rows.iter().enumerate() {
                let normalized = db
                    .normalize_row(table_name, row, tc.strict)
                    .map_err(|e| {
                        Error::new(Status::GenericFailure, format!("Schema error: {}", e))
                    })?;
                db.insert_row(table_name, &normalized, &rel_path, row_index)
                    .map_err(|e| {
                        Error::new(Status::GenericFailure, format!("Insert error: {}", e))
                    })?;
                value_rows.push(normalized);
            }
            file_rows.insert(rel_path, (table_name.clone(), value_rows));
        }

        Ok(DirSQL {
            db: Mutex::new(db),
            root: root_path,
            table_configs,
            ignore_patterns: ignore.unwrap_or_default(),
            file_rows: Mutex::new(file_rows),
            watcher: Mutex::new(None),
        })
    }

    /// Execute a SQL query and return results as an array of objects.
    #[napi]
    pub fn query(&self, sql: String) -> Result<Vec<HashMap<String, serde_json::Value>>> {
        let db = self
            .db
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, format!("Lock error: {}", e)))?;
        let rows = db
            .query(&sql)
            .map_err(|e| Error::new(Status::GenericFailure, format!("Query error: {}", e)))?;

        Ok(rows.iter().map(value_row_to_json).collect())
    }

    /// Start the file watcher. Must be called before pollEvents.
    #[napi(js_name = "startWatcher")]
    pub fn start_watcher(&self) -> Result<()> {
        let watcher = Watcher::new(&self.root)
            .map_err(|e| Error::new(Status::GenericFailure, format!("Watcher error: {}", e)))?;
        let mut guard = self
            .watcher
            .lock()
            .map_err(|e| Error::new(Status::GenericFailure, format!("Lock error: {}", e)))?;
        *guard = Some(watcher);
        Ok(())
    }

    /// Poll for file events with a timeout (in milliseconds).
    /// Returns an array of RowEvent objects, possibly empty if no events within timeout.
    #[napi(js_name = "pollEvents")]
    pub fn poll_events(&self, env: Env, timeout_ms: u32) -> Result<Vec<RowEvent>> {
        let file_events = {
            let guard = self
                .watcher
                .lock()
                .map_err(|e| Error::new(Status::GenericFailure, format!("Lock error: {}", e)))?;
            let watcher = guard.as_ref().ok_or_else(|| {
                Error::new(
                    Status::GenericFailure,
                    "Watcher not started. Call startWatcher first.",
                )
            })?;

            let mut events = Vec::new();
            if let Some(event) = watcher.recv_timeout(Duration::from_millis(timeout_ms as u64)) {
                events.push(event);
                events.extend(watcher.try_recv_all());
            }
            events
        };

        if file_events.is_empty() {
            return Ok(Vec::new());
        }

        let mappings: Vec<(&str, &str)> = self
            .table_configs
            .iter()
            .map(|tc| (tc.glob.as_str(), tc.name.as_str()))
            .collect();
        let ignore_strs: Vec<&str> = self.ignore_patterns.iter().map(|s| s.as_str()).collect();
        let matcher = TableMatcher::new(&mappings, &ignore_strs)
            .map_err(|e| Error::new(Status::GenericFailure, format!("Glob error: {}", e)))?;

        let mut result_events = Vec::new();

        for file_event in file_events {
            let abs_path = match &file_event {
                FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => p,
            };

            let rel_for_match = abs_path.strip_prefix(&self.root).unwrap_or(abs_path);

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
                    let mut file_rows = self.file_rows.lock().map_err(|e| {
                        Error::new(Status::GenericFailure, format!("Lock error: {}", e))
                    })?;
                    let old_entry = file_rows.remove(&rel_path);
                    let old_rows = old_entry.as_ref().map(|(_, rows)| rows.as_slice());

                    let row_events = differ::diff(&table_name, old_rows, None, &rel_path);

                    let db = self.db.lock().map_err(|e| {
                        Error::new(Status::GenericFailure, format!("Lock error: {}", e))
                    })?;
                    db.delete_rows_by_file(&table_name, &rel_path).map_err(|e| {
                        Error::new(Status::GenericFailure, format!("DB error: {}", e))
                    })?;

                    for re in row_events {
                        result_events.push(row_event_to_js(&re, &rel_path));
                    }
                }
                FileEvent::Created(_) | FileEvent::Modified(_) => {
                    let content = match std::fs::read_to_string(abs_path) {
                        Ok(c) => c,
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::NotFound {
                                continue;
                            }
                            return Err(Error::new(
                                Status::GenericFailure,
                                format!("Failed to read {}: {}", abs_path.display(), e),
                            ));
                        }
                    };

                    let tc = match self.table_configs.iter().find(|tc| tc.name == table_name) {
                        Some(tc) => tc,
                        None => continue,
                    };

                    let extract_result = call_extract(&env, &tc.extract_ref, &rel_path, &content);

                    let new_rows = match extract_result {
                        Ok(raw_rows) => {
                            let db = self.db.lock().map_err(|e| {
                                Error::new(Status::GenericFailure, format!("Lock error: {}", e))
                            })?;
                            let mut value_rows = Vec::new();
                            for raw_row in &raw_rows {
                                match db.normalize_row(&table_name, raw_row, tc.strict) {
                                    Ok(r) => value_rows.push(r),
                                    Err(e) => {
                                        result_events.push(RowEvent {
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
                            result_events.push(RowEvent {
                                table: table_name.clone(),
                                action: "error".to_string(),
                                row: None,
                                old_row: None,
                                error: Some(format!("Extract error: {}", e)),
                                file_path: Some(rel_path.clone()),
                            });
                            continue;
                        }
                    };

                    let mut file_rows = self.file_rows.lock().map_err(|e| {
                        Error::new(Status::GenericFailure, format!("Lock error: {}", e))
                    })?;
                    let old_entry = file_rows.get(&rel_path);
                    let old_rows = old_entry.map(|(_, rows)| rows.as_slice());

                    let row_events =
                        differ::diff(&table_name, old_rows, Some(&new_rows), &rel_path);

                    {
                        let db = self.db.lock().map_err(|e| {
                            Error::new(Status::GenericFailure, format!("Lock error: {}", e))
                        })?;
                        db.delete_rows_by_file(&table_name, &rel_path).map_err(|e| {
                            Error::new(Status::GenericFailure, format!("DB error: {}", e))
                        })?;
                        for (row_index, row) in new_rows.iter().enumerate() {
                            db.insert_row(&table_name, row, &rel_path, row_index)
                                .map_err(|e| {
                                    Error::new(
                                        Status::GenericFailure,
                                        format!("Insert error: {}", e),
                                    )
                                })?;
                        }
                    }

                    file_rows.insert(rel_path.clone(), (table_name.clone(), new_rows));

                    for re in row_events {
                        result_events.push(row_event_to_js(&re, &rel_path));
                    }
                }
            }
        }

        Ok(result_events)
    }
}

/// Convert a differ::RowEvent into a JS RowEvent.
fn row_event_to_js(event: &differ::RowEvent, rel_path: &str) -> RowEvent {
    match event {
        differ::RowEvent::Insert { table, row } => RowEvent {
            table: table.clone(),
            action: "insert".to_string(),
            row: Some(value_row_to_json(row)),
            old_row: None,
            error: None,
            file_path: Some(rel_path.to_string()),
        },
        differ::RowEvent::Update {
            table,
            old_row,
            new_row,
        } => RowEvent {
            table: table.clone(),
            action: "update".to_string(),
            row: Some(value_row_to_json(new_row)),
            old_row: Some(value_row_to_json(old_row)),
            error: None,
            file_path: Some(rel_path.to_string()),
        },
        differ::RowEvent::Delete { table, row } => RowEvent {
            table: table.clone(),
            action: "delete".to_string(),
            row: Some(value_row_to_json(row)),
            old_row: None,
            error: None,
            file_path: Some(rel_path.to_string()),
        },
        differ::RowEvent::Error { file_path, error } => RowEvent {
            table: String::new(),
            action: "error".to_string(),
            row: None,
            old_row: None,
            error: Some(error.clone()),
            file_path: Some(file_path.to_string_lossy().to_string()),
        },
    }
}
