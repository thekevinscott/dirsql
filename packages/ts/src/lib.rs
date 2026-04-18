// The raw napi_sys helpers below are already declared `unsafe fn` as a
// whole. Edition 2024 adds a lint that requires each unsafe op to be
// wrapped in its own `unsafe { }` block; that would only add noise here.
#![allow(unsafe_op_in_unsafe_fn)]

//! napi-rs binding for `dirsql`. Intentionally thin: all orchestration lives
//! in `dirsql::DirSQL`. This layer only:
//!
//! - wraps a JS `extract` callable in a Rust closure (via a persistent napi
//!   reference) so it can be handed to [`dirsql::Table`]
//! - converts row values and events between Rust and serde_json shapes napi
//!   exposes to JS
//! - forwards the constructor / `fromConfig` / `query` / `startWatcher` /
//!   `pollEvents` to the corresponding `DirSQL` methods
//!
//! All methods execute on the JS event-loop thread, so the JS `extract`
//! callback is only ever invoked synchronously from that thread.

use dirsql::{DirSQL as CoreDirSQL, RawFileEvent, Row, RowEvent as CoreRowEvent, Table, Value};
use napi::bindgen_prelude::*;
use napi::Task;
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// -- Public napi-rs classes --------------------------------------------------

/// A row-level event emitted by the file watcher.
///
/// `table` is nullable because error events may occur before a file has
/// been attributed to any table (e.g. a watch-channel failure). For
/// insert / update / delete events it is always set.
#[napi(object)]
pub struct RowEvent {
    pub table: Option<String>,
    pub action: String,
    pub row: Option<HashMap<String, serde_json::Value>>,
    pub old_row: Option<HashMap<String, serde_json::Value>>,
    pub error: Option<String>,
    pub file_path: Option<String>,
}

/// The main DirSQL class. Creates an in-memory SQLite index over a directory.
#[napi(js_name = "DirSQL")]
pub struct DirSQL {
    inner: CoreDirSQL,
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
        let tables_len = tables.len();
        let mut rust_tables: Vec<Table> = Vec::with_capacity(tables_len as usize);

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

            let fn_ref = unsafe { Arc::new(FnRef::new(raw_env, extract_val)?) };
            let mut table = Table::try_new(ddl, glob, make_extract_closure(fn_ref));
            table.strict = strict;
            rust_tables.push(table);
        }

        let inner = match ignore {
            Some(ig) => CoreDirSQL::with_ignore(root, rust_tables, ig),
            None => CoreDirSQL::new(root, rust_tables),
        }
        .map_err(to_napi_err)?;

        Ok(DirSQL { inner })
    }

    /// Create a DirSQL instance from a `.dirsql.toml` config file.
    ///
    /// Parses the TOML config, derives the root directory from the config
    /// file's parent, and scans files using the built-in parser for each
    /// declared format. No JS `extract` callback is required.
    #[napi(factory, js_name = "fromConfig")]
    pub fn from_config(config_path: String) -> Result<Self> {
        let inner = CoreDirSQL::from_config_path(&config_path).map_err(to_napi_err)?;
        Ok(DirSQL { inner })
    }

    /// Execute a SQL query and return results as an array of objects.
    ///
    /// Runs on the libuv threadpool so queries don't block the JS event loop.
    /// Returns a `Promise` in JS.
    #[napi(ts_return_type = "Promise<Record<string, unknown>[]>")]
    pub fn query(&self, sql: String) -> AsyncTask<QueryTask> {
        AsyncTask::new(QueryTask {
            inner: self.inner.clone(),
            sql,
        })
    }

    /// Start the file watcher. Must be called before pollEvents.
    ///
    /// Runs on the libuv threadpool so the JS event loop stays responsive
    /// while the watcher is being initialized. Returns a `Promise` in JS.
    #[napi(js_name = "startWatcher", ts_return_type = "Promise<void>")]
    pub fn start_watcher(&self) -> AsyncTask<StartWatcherTask> {
        AsyncTask::new(StartWatcherTask {
            inner: self.inner.clone(),
        })
    }

    /// Poll for file events with a timeout (in milliseconds).
    /// Returns an array of RowEvent objects, possibly empty.
    ///
    /// Runs on the libuv threadpool so the JS event loop stays responsive
    /// for the duration of the poll timeout. Returns a `Promise` in JS.
    #[napi(js_name = "pollEvents", ts_return_type = "Promise<RowEvent[]>")]
    pub fn poll_events(&self, timeout_ms: u32) -> AsyncTask<PollEventsTask> {
        AsyncTask::new(PollEventsTask {
            inner: self.inner.clone(),
            timeout_ms,
        })
    }
}

// -- Async tasks -------------------------------------------------------------

/// Runs `DirSQL::query` on the libuv threadpool so the JS event loop stays
/// responsive. `CoreDirSQL` is cheap to clone (internally `Arc`-wrapped), so
/// each task owns its own handle for the lifetime of the query.
pub struct QueryTask {
    inner: CoreDirSQL,
    sql: String,
}

impl Task for QueryTask {
    type Output = Vec<HashMap<String, serde_json::Value>>;
    type JsValue = Vec<HashMap<String, serde_json::Value>>;

    fn compute(&mut self) -> Result<Self::Output> {
        let rows = self.inner.query(&self.sql).map_err(to_napi_err)?;
        Ok(rows.iter().map(value_row_to_json).collect())
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// Runs `DirSQL::start_watching` on the libuv threadpool. Idempotent on the
/// core side, so repeated calls from JS are still safe.
pub struct StartWatcherTask {
    inner: CoreDirSQL,
}

impl Task for StartWatcherTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> Result<Self::Output> {
        self.inner.start_watching().map_err(to_napi_err)
    }

    fn resolve(&mut self, _env: Env, _output: Self::Output) -> Result<Self::JsValue> {
        Ok(())
    }
}

/// Splits polling across the libuv threadpool and the JS main thread.
///
/// The blocking wait for raw file events runs in `compute()` on the
/// threadpool (parking a worker thread, not the JS thread). Processing
/// those events into [`RowEvent`]s — which invokes the JS `extract`
/// callback for created / modified files — runs in `resolve()` on the
/// JS main thread, where napi handles are valid. Without this split,
/// `compute()` would call into JS from a worker thread and crash V8
/// with "Cannot create a handle without a HandleScope".
pub struct PollEventsTask {
    inner: CoreDirSQL,
    timeout_ms: u32,
}

impl Task for PollEventsTask {
    type Output = Vec<RawFileEvent>;
    type JsValue = Vec<RowEvent>;

    fn compute(&mut self) -> Result<Self::Output> {
        self.inner
            .wait_file_events(Duration::from_millis(self.timeout_ms as u64))
            .map_err(to_napi_err)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        let row_events = self.inner.apply_file_events(output);
        Ok(row_events.iter().map(row_event_to_js).collect())
    }
}

// -- JS callback plumbing ----------------------------------------------------

/// A persistent reference to a JS function, safe to store across calls.
///
/// SAFETY: All access happens on the JS main thread via `#[napi]` methods.
/// `DirSQL::new` and `DirSQL::pollEvents` both run on that thread, and the
/// extract closure is only invoked synchronously within those methods.
struct FnRef {
    raw_env: napi::sys::napi_env,
    raw_ref: napi::sys::napi_ref,
}

unsafe impl Send for FnRef {}
unsafe impl Sync for FnRef {}

impl FnRef {
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

    unsafe fn call_extract(
        &self,
        rel_path: &str,
        content: &str,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let env = self.raw_env;
        let func = self.get_value()?;

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

        let mut undefined = std::ptr::null_mut();
        napi::sys::napi_get_undefined(env, &mut undefined);

        let args = [js_path, js_content];
        let mut result = std::ptr::null_mut();
        let status =
            napi::sys::napi_call_function(env, undefined, func, 2, args.as_ptr(), &mut result);
        if status != napi::sys::Status::napi_ok {
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

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

fn make_extract_closure(
    fn_ref: Arc<FnRef>,
) -> impl Fn(&str, &str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static {
    move |path: &str, content: &str| unsafe {
        fn_ref
            .call_extract(path, content)
            .map_err(|e| -> BoxError { Box::new(ExtractError(e.to_string())) })
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

fn to_napi_err<E: std::fmt::Display>(e: E) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}

// -- JS <-> Rust value conversion helpers ------------------------------------

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

        let mut names = std::ptr::null_mut();
        napi::sys::napi_get_property_names(env, element, &mut names);

        let mut names_len: u32 = 0;
        napi::sys::napi_get_array_length(env, names, &mut names_len);

        let mut row = HashMap::new();

        for j in 0..names_len {
            let mut key_val = std::ptr::null_mut();
            napi::sys::napi_get_element(env, names, j, &mut key_val);

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

            let mut val = std::ptr::null_mut();
            napi::sys::napi_get_property(env, element, key_val, &mut val);

            let value = js_val_to_value(env, val)?;
            row.insert(key, value);
        }

        rows.push(row);
    }

    Ok(rows)
}

unsafe fn js_val_to_value(env: napi::sys::napi_env, val: napi::sys::napi_value) -> Result<Value> {
    let mut value_type = 0i32;
    napi::sys::napi_typeof(env, val, &mut value_type);

    match value_type {
        0 | 1 => Ok(Value::Null),
        2 => {
            let mut b = false;
            napi::sys::napi_get_value_bool(env, val, &mut b);
            Ok(Value::Integer(if b { 1 } else { 0 }))
        }
        3 => {
            let mut n: f64 = 0.0;
            napi::sys::napi_get_value_double(env, val, &mut n);
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                Ok(Value::Integer(n as i64))
            } else {
                Ok(Value::Real(n))
            }
        }
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
        return default;
    }

    let mut b = default;
    napi::sys::napi_get_value_bool(env, val, &mut b);
    b
}

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
    if value_type != 7 {
        return Err(Error::new(
            Status::GenericFailure,
            format!("Property '{}' must be a function", name),
        ));
    }

    Ok(val)
}

// -- Row/event conversion ----------------------------------------------------

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

fn value_row_to_json(row: &HashMap<String, Value>) -> HashMap<String, serde_json::Value> {
    row.iter()
        .map(|(k, v)| (k.clone(), value_to_json(v)))
        .collect()
}

fn row_event_to_js(event: &CoreRowEvent) -> RowEvent {
    match event {
        CoreRowEvent::Insert {
            table,
            row,
            file_path,
        } => RowEvent {
            table: Some(table.clone()),
            action: "insert".to_string(),
            row: Some(value_row_to_json(row)),
            old_row: None,
            error: None,
            file_path: Some(file_path.clone()),
        },
        CoreRowEvent::Update {
            table,
            old_row,
            new_row,
            file_path,
        } => RowEvent {
            table: Some(table.clone()),
            action: "update".to_string(),
            row: Some(value_row_to_json(new_row)),
            old_row: Some(value_row_to_json(old_row)),
            error: None,
            file_path: Some(file_path.clone()),
        },
        CoreRowEvent::Delete {
            table,
            row,
            file_path,
        } => RowEvent {
            table: Some(table.clone()),
            action: "delete".to_string(),
            row: Some(value_row_to_json(row)),
            old_row: None,
            error: None,
            file_path: Some(file_path.clone()),
        },
        CoreRowEvent::Error {
            table,
            file_path,
            error,
        } => RowEvent {
            table: table.clone(),
            action: "error".to_string(),
            row: None,
            old_row: None,
            error: Some(error.clone()),
            file_path: Some(file_path.to_string_lossy().to_string()),
        },
    }
}
