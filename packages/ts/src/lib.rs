use dirsql_core::db::{parse_table_name, Db, Value};
use dirsql_core::differ;
use dirsql_core::matcher::TableMatcher;
use dirsql_core::scanner::scan_directory;
use dirsql_core::watcher::{FileEvent, Watcher};
use napi::bindgen_prelude::*;
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

/// A persistent reference to a JS function, safe to store across calls.
///
/// Uses napi_sys raw reference counting to persist JS function values.
/// SAFETY: All access happens on the JS main thread via #[napi] methods.
struct FnRef {
    raw_env: napi::sys::napi_env,
    raw_ref: napi::sys::napi_ref,
}

unsafe impl Send for FnRef {}
unsafe impl Sync for FnRef {}

impl FnRef {
    /// Create a persistent reference from a raw napi_value (must be a function).
    unsafe fn new(env: napi::sys::napi_env, value: napi::sys::napi_value) -> Result<Self> {
        let mut raw_ref = std::ptr::null_mut();
        let status = napi::sys::napi_create_reference(env, value, 1, &mut raw_ref);
        if status != napi::sys::Status::napi_ok {
            return Err(Error::new(
                Status::GenericFailure,
                "Failed to create reference",
            ));
        }
        Ok(FnRef {
            raw_env: env,
            raw_ref,
        })
    }

    /// Get the referenced value.
    unsafe fn get_value(&self) -> Result<napi::sys::napi_value> {
        let mut result = std::ptr::null_mut();
        let status = napi::sys::napi_get_reference_value(self.raw_env, self.raw_ref, &mut result);
        if status != napi::sys::Status::napi_ok {
            return Err(Error::new(
                Status::GenericFailure,
                "Failed to get reference value",
            ));
        }
        Ok(result)
    }

    /// Call this function reference with (filePath, content) args and return the result as JSON.
    unsafe fn call_extract(
        &self,
        rel_path: &str,
        content: &str,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let env = self.raw_env;
        let func = self.get_value()?;

        // Create string args
        let mut js_path = std::ptr::null_mut();
        let status = napi::sys::napi_create_string_utf8(
            env,
            rel_path.as_ptr() as *const _,
            rel_path.len() as isize,
            &mut js_path,
        );
        if status != napi::sys::Status::napi_ok {
            return Err(Error::new(
                Status::GenericFailure,
                "Failed to create path string",
            ));
        }

        let mut js_content = std::ptr::null_mut();
        let status = napi::sys::napi_create_string_utf8(
            env,
            content.as_ptr() as *const _,
            content.len() as isize,
            &mut js_content,
        );
        if status != napi::sys::Status::napi_ok {
            return Err(Error::new(
                Status::GenericFailure,
                "Failed to create content string",
            ));
        }

        // Get undefined for 'this'
        let mut undefined = std::ptr::null_mut();
        napi::sys::napi_get_undefined(env, &mut undefined);

        // Call the function
        let args = [js_path, js_content];
        let mut result = std::ptr::null_mut();
        let status =
            napi::sys::napi_call_function(env, undefined, func, 2, args.as_ptr(), &mut result);
        if status != napi::sys::Status::napi_ok {
            // Check for pending exception
            let mut is_exception = false;
            napi::sys::napi_is_exception_pending(env, &mut is_exception);
            if is_exception {
                let mut exception = std::ptr::null_mut();
                napi::sys::napi_get_and_clear_last_exception(env, &mut exception);
            }
            return Err(Error::new(
                Status::GenericFailure,
                "Extract function call failed",
            ));
        }

        // Parse result array
        parse_js_array_of_objects(env, result)
    }
}

impl Drop for FnRef {
    fn drop(&mut self) {
        unsafe {
            napi::sys::napi_delete_reference(self.raw_env, self.raw_ref);
        }
    }
}

/// Parse a JS array of objects into Vec<HashMap<String, Value>> using napi_sys.
unsafe fn parse_js_array_of_objects(
    env: napi::sys::napi_env,
    array: napi::sys::napi_value,
) -> Result<Vec<HashMap<String, Value>>> {
    let mut is_array = false;
    napi::sys::napi_is_array(env, array, &mut is_array);
    if !is_array {
        return Err(Error::new(
            Status::GenericFailure,
            "Extract must return an array",
        ));
    }

    let mut length: u32 = 0;
    napi::sys::napi_get_array_length(env, array, &mut length);

    let mut rows = Vec::with_capacity(length as usize);

    for i in 0..length {
        let mut element = std::ptr::null_mut();
        napi::sys::napi_get_element(env, array, i, &mut element);

        // Get property names
        let mut names = std::ptr::null_mut();
        napi::sys::napi_get_property_names(env, element, &mut names);

        let mut names_len: u32 = 0;
        napi::sys::napi_get_array_length(env, names, &mut names_len);

        let mut row = HashMap::new();

        for j in 0..names_len {
            let mut key_val = std::ptr::null_mut();
            napi::sys::napi_get_element(env, names, j, &mut key_val);

            // Get key string
            let mut key_len = 0usize;
            napi::sys::napi_get_value_string_utf8(
                env,
                key_val,
                std::ptr::null_mut(),
                0,
                &mut key_len,
            );
            let mut key_buf = vec![0u8; key_len + 1];
            let mut actual_len = 0usize;
            napi::sys::napi_get_value_string_utf8(
                env,
                key_val,
                key_buf.as_mut_ptr() as *mut _,
                key_len + 1,
                &mut actual_len,
            );
            let key = String::from_utf8_lossy(&key_buf[..actual_len]).to_string();

            // Get value
            let mut val = std::ptr::null_mut();
            napi::sys::napi_get_property(env, element, key_val, &mut val);

            let value = js_val_to_value(env, val)?;
            row.insert(key, value);
        }

        rows.push(row);
    }

    Ok(rows)
}

/// Convert a raw napi_value to a dirsql_core Value.
unsafe fn js_val_to_value(env: napi::sys::napi_env, val: napi::sys::napi_value) -> Result<Value> {
    let mut value_type = 0i32;
    napi::sys::napi_typeof(env, val, &mut value_type);

    match value_type {
        // napi_undefined = 0, napi_null = 1
        0 | 1 => Ok(Value::Null),
        // napi_boolean = 2
        2 => {
            let mut b = false;
            napi::sys::napi_get_value_bool(env, val, &mut b);
            Ok(Value::Integer(if b { 1 } else { 0 }))
        }
        // napi_number = 3
        3 => {
            let mut n: f64 = 0.0;
            napi::sys::napi_get_value_double(env, val, &mut n);
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                Ok(Value::Integer(n as i64))
            } else {
                Ok(Value::Real(n))
            }
        }
        // napi_string = 4
        4 => {
            let mut len = 0usize;
            napi::sys::napi_get_value_string_utf8(env, val, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len + 1];
            let mut actual = 0usize;
            napi::sys::napi_get_value_string_utf8(
                env,
                val,
                buf.as_mut_ptr() as *mut _,
                len + 1,
                &mut actual,
            );
            Ok(Value::Text(
                String::from_utf8_lossy(&buf[..actual]).to_string(),
            ))
        }
        // Everything else -> stringify
        _ => {
            let mut str_val = std::ptr::null_mut();
            let status = napi::sys::napi_coerce_to_string(env, val, &mut str_val);
            if status != napi::sys::Status::napi_ok {
                return Ok(Value::Text("[object]".to_string()));
            }
            let mut len = 0usize;
            napi::sys::napi_get_value_string_utf8(env, str_val, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len + 1];
            let mut actual = 0usize;
            napi::sys::napi_get_value_string_utf8(
                env,
                str_val,
                buf.as_mut_ptr() as *mut _,
                len + 1,
                &mut actual,
            );
            Ok(Value::Text(
                String::from_utf8_lossy(&buf[..actual]).to_string(),
            ))
        }
    }
}

/// Get a string property from a JS object.
unsafe fn get_string_property(
    env: napi::sys::napi_env,
    obj: napi::sys::napi_value,
    name: &str,
) -> Result<String> {
    let mut key = std::ptr::null_mut();
    napi::sys::napi_create_string_utf8(
        env,
        name.as_ptr() as *const _,
        name.len() as isize,
        &mut key,
    );

    let mut has = false;
    napi::sys::napi_has_property(env, obj, key, &mut has);
    if !has {
        return Err(Error::new(
            Status::GenericFailure,
            format!("Missing property: {}", name),
        ));
    }

    let mut val = std::ptr::null_mut();
    napi::sys::napi_get_property(env, obj, key, &mut val);

    let mut len = 0usize;
    napi::sys::napi_get_value_string_utf8(env, val, std::ptr::null_mut(), 0, &mut len);
    let mut buf = vec![0u8; len + 1];
    let mut actual = 0usize;
    napi::sys::napi_get_value_string_utf8(
        env,
        val,
        buf.as_mut_ptr() as *mut _,
        len + 1,
        &mut actual,
    );
    Ok(String::from_utf8_lossy(&buf[..actual]).to_string())
}

/// Get a bool property from a JS object, with default.
unsafe fn get_bool_property(
    env: napi::sys::napi_env,
    obj: napi::sys::napi_value,
    name: &str,
    default: bool,
) -> bool {
    let mut key = std::ptr::null_mut();
    napi::sys::napi_create_string_utf8(
        env,
        name.as_ptr() as *const _,
        name.len() as isize,
        &mut key,
    );

    let mut has = false;
    napi::sys::napi_has_property(env, obj, key, &mut has);
    if !has {
        return default;
    }

    let mut val = std::ptr::null_mut();
    napi::sys::napi_get_property(env, obj, key, &mut val);

    let mut value_type = 0i32;
    napi::sys::napi_typeof(env, val, &mut value_type);
    if value_type != 2 {
        // not a boolean
        return default;
    }

    let mut b = default;
    napi::sys::napi_get_value_bool(env, val, &mut b);
    b
}

/// Get a function property from a JS object.
unsafe fn get_function_property(
    env: napi::sys::napi_env,
    obj: napi::sys::napi_value,
    name: &str,
) -> Result<napi::sys::napi_value> {
    let mut key = std::ptr::null_mut();
    napi::sys::napi_create_string_utf8(
        env,
        name.as_ptr() as *const _,
        name.len() as isize,
        &mut key,
    );

    let mut has = false;
    napi::sys::napi_has_property(env, obj, key, &mut has);
    if !has {
        return Err(Error::new(
            Status::GenericFailure,
            format!("Missing property: {}", name),
        ));
    }

    let mut val = std::ptr::null_mut();
    napi::sys::napi_get_property(env, obj, key, &mut val);

    let mut value_type = 0i32;
    napi::sys::napi_typeof(env, val, &mut value_type);
    if value_type != 6 {
        // napi_function = 6
        return Err(Error::new(
            Status::GenericFailure,
            format!("Property '{}' must be a function", name),
        ));
    }

    Ok(val)
}

/// Internal table config stored after init for use during watch.
struct TableConfig {
    name: String,
    glob: String,
    extract_ref: FnRef,
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
    #[allow(clippy::type_complexity)]
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

#[napi]
impl DirSQL {
    /// Create a new DirSQL instance.
    ///
    /// @param root - Root directory path to index
    /// @param tables - Array of table definition objects
    /// @param ignore - Optional array of glob patterns to ignore
    #[napi(constructor)]
    #[allow(clippy::new_ret_no_self)]
    pub fn new(env: Env, root: String, tables: Array, ignore: Option<Vec<String>>) -> Result<Self> {
        let raw_env = env.raw();
        let db = Db::new()
            .map_err(|e| Error::new(Status::GenericFailure, format!("DB init error: {}", e)))?;

        let root_path = PathBuf::from(&root);
        let mut table_configs: Vec<TableConfig> = Vec::new();

        let tables_len = tables.len();

        for i in 0..tables_len {
            let table_element: Unknown<'_> = tables.get(i)?.ok_or_else(|| {
                Error::new(
                    Status::GenericFailure,
                    format!("Missing table at index {}", i),
                )
            })?;
            let raw_obj = table_element.raw();

            let ddl = unsafe { get_string_property(raw_env, raw_obj, "ddl")? };
            let glob = unsafe { get_string_property(raw_env, raw_obj, "glob")? };
            let extract_val = unsafe { get_function_property(raw_env, raw_obj, "extract")? };
            let strict = unsafe { get_bool_property(raw_env, raw_obj, "strict", false) };

            let table_name = parse_table_name(&ddl).ok_or_else(|| {
                Error::new(
                    Status::GenericFailure,
                    format!("Could not parse table name from DDL: {}", ddl),
                )
            })?;
            db.create_table(&ddl)
                .map_err(|e| Error::new(Status::GenericFailure, format!("DDL error: {}", e)))?;

            let extract_ref = unsafe { FnRef::new(raw_env, extract_val)? };

            table_configs.push(TableConfig {
                name: table_name,
                glob,
                extract_ref,
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

        // Process each file - call extract directly since we're on the JS thread
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

            let rows = unsafe { tc.extract_ref.call_extract(&rel_path, &content)? };

            let mut value_rows: Vec<HashMap<String, Value>> = Vec::new();
            for (row_index, row) in rows.iter().enumerate() {
                let normalized = db.normalize_row(table_name, row, tc.strict).map_err(|e| {
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
    pub fn poll_events(&self, timeout_ms: u32) -> Result<Vec<RowEvent>> {
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
                    db.delete_rows_by_file(&table_name, &rel_path)
                        .map_err(|e| {
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

                    // Call extract via stored function reference
                    let extract_result =
                        unsafe { tc.extract_ref.call_extract(&rel_path, &content) };

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
                        db.delete_rows_by_file(&table_name, &rel_path)
                            .map_err(|e| {
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
