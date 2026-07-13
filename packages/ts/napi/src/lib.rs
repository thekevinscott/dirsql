// The raw napi_sys helpers below are already declared `unsafe fn` as a
// whole. Edition 2024 adds a lint that requires each unsafe op to be
// wrapped in its own `unsafe { }` block; that would only add noise here.
#![allow(unsafe_op_in_unsafe_fn)]

//! napi-rs binding for `dirsql`. Intentionally thin: all orchestration lives
//! in `dirsql::DirSQL`. This layer only:
//!
//! - wraps a JS `onFile` callable in a Rust closure (via a persistent napi
//!   reference) so it can be handed to [`dirsql::Table`]
//! - converts row values and events between Rust and the napi shapes exposed
//!   to JS (BLOB columns cross as Node `Buffer`s via [`JsRowValue`])
//! - forwards `openAsync` / `query` / `startWatcher` / `pollEvents` to the
//!   corresponding `DirSQL` methods
//!
//! `openAsync` is the single construction entry point. It accepts an optional
//! `root`, optional `tables`, optional `ignore`, and optional `config` path;
//! the TS wrapper exposes a matching overloaded constructor so callers can
//! write either `new DirSQL(configPath)` or `new DirSQL({ root, tables, ... })`.

use dirsql::{
    DirSQL as CoreDirSQL, Extension, PreparedBuild, RawFileEvent, Row, RowEvent as CoreRowEvent,
    Table, Value, db::parse_table_name as core_parse_table_name,
};
use napi::Task;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Parse the table name out of a `CREATE TABLE <name> (...)` DDL.
/// Returns `null` if the DDL doesn't match (e.g. empty, missing
/// `CREATE TABLE`, or the identifier slot is empty).
#[napi(js_name = "parseTableName")]
pub fn parse_table_name(ddl: String) -> Option<String> {
    core_parse_table_name(&ddl)
}

/// A row-level event emitted by the file watcher.
///
/// `table` is nullable because error events may occur before a file has
/// been attributed to any table (e.g. a watch-channel failure). For
/// insert / update / delete events it is always set.
///
/// Output-only (`object_from_js = false`): JS never constructs one, so
/// [`JsRowValue`] only needs the Rust -> JS direction.
#[napi(object, object_from_js = false)]
pub struct RowEvent {
    pub table: Option<String>,
    pub action: String,
    #[napi(ts_type = "Record<string, unknown>")]
    pub row: Option<HashMap<String, JsRowValue>>,
    #[napi(ts_type = "Record<string, unknown>")]
    pub old_row: Option<HashMap<String, JsRowValue>>,
    pub error: Option<String>,
    pub file_path: Option<String>,
}

/// A SQLite extension to load at startup, marshaled from the JS
/// `{ path, entrypoint? }` object into a [`dirsql::Extension`]. Paths are
/// taken verbatim — the programmatic surface does not resolve relative
/// paths.
#[napi(object)]
pub struct ExtensionSpec {
    pub path: String,
    pub entrypoint: Option<String>,
}

/// The main DirSQL class. Creates an ephemeral SQLite index over a directory.
#[napi(js_name = "DirSQL")]
pub struct DirSQL {
    inner: CoreDirSQL,
}

#[napi]
impl DirSQL {
    /// The single construction entry point. All arguments are optional; at
    /// least one of `root` or `config` must be provided.
    ///
    /// Table parsing runs synchronously on the JS thread (napi references to
    /// each JS `onFile` callback can only be created there). The directory
    /// scan + file I/O then runs on the libuv threadpool via [`OpenTask`];
    /// the `onFile` callbacks and DB inserts run back on the JS thread in
    /// the task's `resolve` phase.
    ///
    /// When `config` is supplied, its `[[table]]` entries are appended after
    /// any programmatic `tables` and its `[dirsql].ignore` is appended after
    /// any explicit `ignore`. When both `root` and config's `[dirsql].root`
    /// are supplied, the explicit `root` wins and a warning is emitted.
    ///
    /// `suppress_config_extensions` skips the core's own loading of the
    /// config's `[[dirsql.extension]]` entries; the TS wrapper sets it after
    /// resolving those entries itself (package names need `require.resolve`,
    /// which the core lacks) and passing the resolved literal paths via
    /// `extensions`, so the entries are not loaded twice.
    #[allow(clippy::too_many_arguments)]
    #[napi(js_name = "openAsync", ts_return_type = "Promise<DirSQL>")]
    pub fn open_async(
        env: Env,
        root: Option<String>,
        tables: Option<Array>,
        ignore: Option<Vec<String>>,
        config: Option<Vec<String>>,
        persist: Option<bool>,
        persist_path: Option<String>,
        extensions: Option<Vec<ExtensionSpec>>,
        suppress_config_extensions: Option<bool>,
    ) -> Result<AsyncTask<OpenTask>> {
        let rust_tables = match tables {
            Some(ts) => parse_tables_from_js(env, ts)?,
            None => Vec::new(),
        };
        let rust_extensions = extensions
            .unwrap_or_default()
            .into_iter()
            .map(|e| Extension {
                path: PathBuf::from(e.path),
                entrypoint: e.entrypoint,
            })
            .collect();
        Ok(AsyncTask::new(OpenTask {
            root: root.map(PathBuf::from),
            config_paths: config
                .unwrap_or_default()
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            tables: Some(rust_tables),
            ignore: ignore.unwrap_or_default(),
            persist: persist.unwrap_or(false),
            persist_path: persist_path.map(PathBuf::from),
            extensions: rust_extensions,
            suppress_config_extensions: suppress_config_extensions.unwrap_or(false),
        }))
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

/// Splits construction across the libuv threadpool and the JS main thread.
///
/// `compute()` resolves the builder (loading a `.dirsql.toml` if supplied)
/// and performs the directory scan + file reads via the builder's
/// `prepare()` — I/O that is safe to run on a worker thread. `resolve()`
/// then runs the `onFile` callbacks and DB inserts via
/// [`CoreDirSQL::finish_build`], which must happen on the JS main thread so
/// napi handles remain valid when invoking each JS `onFile` callback.
pub struct OpenTask {
    root: Option<PathBuf>,
    config_paths: Vec<PathBuf>,
    // `Option` so we can move `tables` out in `compute` without requiring
    // `Table: Default` for `std::mem::take`.
    tables: Option<Vec<Table>>,
    ignore: Vec<String>,
    persist: bool,
    persist_path: Option<PathBuf>,
    /// SQLite extensions to load onto the connection before any table DDL.
    /// Config-file `[[dirsql.extension]]` entries are appended by the builder
    /// unless `suppress_config_extensions` is set (the TS wrapper already
    /// resolved and included them).
    extensions: Vec<Extension>,
    suppress_config_extensions: bool,
}

impl Task for OpenTask {
    type Output = PreparedBuild;
    type JsValue = DirSQL;

    fn compute(&mut self) -> Result<Self::Output> {
        let tables = self.tables.take().ok_or_else(|| {
            Error::new(Status::GenericFailure, "OpenTask computed more than once")
        })?;
        let ignore = std::mem::take(&mut self.ignore);
        let extensions = std::mem::take(&mut self.extensions);

        let mut builder = CoreDirSQL::builder()
            .tables(tables)
            .ignore(ignore)
            .extensions(extensions)
            .suppress_config_extensions(self.suppress_config_extensions);
        if let Some(root) = self.root.take() {
            builder = builder.root(root);
        }
        for cfg in std::mem::take(&mut self.config_paths) {
            builder = builder.config(cfg);
        }
        if self.persist {
            builder = builder.persist(self.persist_path.take());
        }
        builder.prepare().map_err(to_napi_err)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        let inner = CoreDirSQL::finish_build(output).map_err(to_napi_err)?;
        Ok(DirSQL { inner })
    }
}

/// Runs `DirSQL::query` on the libuv threadpool so the JS event loop stays
/// responsive. `CoreDirSQL` is cheap to clone (internally `Arc`-wrapped), so
/// each task owns its own handle for the lifetime of the query.
pub struct QueryTask {
    inner: CoreDirSQL,
    sql: String,
}

impl Task for QueryTask {
    type Output = Vec<HashMap<String, JsRowValue>>;
    type JsValue = Vec<HashMap<String, JsRowValue>>;

    fn compute(&mut self) -> Result<Self::Output> {
        let rows = self.inner.query(&self.sql).map_err(to_napi_err)?;
        rows.iter().map(value_row_to_js).collect()
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
/// those events into [`RowEvent`]s — which invokes the JS `onFile`
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
        row_events.iter().map(row_event_to_js).collect()
    }
}

/// Parse a JS array of `TableDef` objects into Rust [`Table`]s. Must run on
/// the JS thread: creates a persistent napi reference to each `onFile`
/// callback so it can be invoked later without a live JS call frame.
fn parse_tables_from_js(env: Env, tables: Array) -> Result<Vec<Table>> {
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
        let on_file_val = unsafe { get_function_property(raw_env, raw_obj, "onFile")? };
        let strict = unsafe { get_bool_property(raw_env, raw_obj, "strict", false) };

        let fn_ref = unsafe { Arc::new(FnRef::new(raw_env, on_file_val)?) };
        let mut table = Table::try_new(ddl, glob, make_on_file_closure(fn_ref));
        table.strict = strict;
        rust_tables.push(table);
    }

    Ok(rust_tables)
}

/// A persistent reference to a JS function, safe to store across calls.
///
/// SAFETY: All access happens on the JS main thread via `#[napi]` methods.
/// `DirSQL::new` and `DirSQL::pollEvents` both run on that thread, and the
/// onFile closure is only invoked synchronously within those methods.
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

    unsafe fn call_on_file(&self, abs_path: &str) -> Result<Vec<HashMap<String, Value>>> {
        let env = self.raw_env;
        let func = self.get_value()?;

        let mut js_path = std::ptr::null_mut();
        let status = napi::sys::napi_create_string_utf8(
            env,
            abs_path.as_ptr() as *const _,
            abs_path.len() as isize,
            &mut js_path,
        );
        if status != napi::sys::Status::napi_ok {
            return Err(Error::new(
                Status::GenericFailure,
                "Failed to create path string",
            ));
        }

        let mut undefined = std::ptr::null_mut();
        napi::sys::napi_get_undefined(env, &mut undefined);

        let args = [js_path];
        let mut result = std::ptr::null_mut();
        let status =
            napi::sys::napi_call_function(env, undefined, func, 1, args.as_ptr(), &mut result);
        if status != napi::sys::Status::napi_ok {
            let mut is_exception = false;
            napi::sys::napi_is_exception_pending(env, &mut is_exception);
            if is_exception {
                let mut exception = std::ptr::null_mut();
                napi::sys::napi_get_and_clear_last_exception(env, &mut exception);
                return Err(Error::new(
                    Status::GenericFailure,
                    extract_exception_message(env, exception),
                ));
            }
            return Err(Error::new(
                Status::GenericFailure,
                "on-file function call failed".to_string(),
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

fn make_on_file_closure(
    fn_ref: Arc<FnRef>,
) -> impl Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static {
    move |path: &str| unsafe {
        fn_ref
            .call_on_file(path)
            .map_err(|e| -> BoxError { Box::new(OnFileError(e.to_string())) })
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

fn to_napi_err<E: std::fmt::Display>(e: E) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}

unsafe fn parse_js_array_of_objects(
    env: napi::sys::napi_env,
    array: napi::sys::napi_value,
) -> Result<Vec<HashMap<String, Value>>> {
    let mut is_array = false;
    napi::sys::napi_is_array(env, array, &mut is_array);
    if !is_array {
        return Err(Error::new(
            Status::GenericFailure,
            "on-file must return an array",
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
        4 => Ok(Value::Text(read_js_string(env, val))),
        // BigInt: an INTEGER within i64, or an explicit range error. Never a
        // silent TEXT fallback (the lossy behavior this replaces).
        9 => {
            let mut result: i64 = 0;
            let mut lossless = false;
            napi::sys::napi_get_value_bigint_int64(env, val, &mut result, &mut lossless);
            if !lossless {
                return Err(Error::new(
                    Status::GenericFailure,
                    format!(
                        "BigInt {} exceeds the i64 range dirsql can store",
                        coerce_js_to_string(env, val)
                    ),
                ));
            }
            Ok(Value::Integer(result))
        }
        _ => {
            // `Buffer` / `Uint8Array` (Buffer is a Uint8Array subclass)
            // marshals to a BLOB; any other object shape falls through to
            // string coercion.
            if let Some(bytes) = get_u8_array_bytes(env, val) {
                return Ok(Value::Blob(bytes));
            }
            Ok(Value::Text(coerce_js_to_string(env, val)))
        }
    }
}

/// Read a JS string value into a Rust `String`.
unsafe fn read_js_string(env: napi::sys::napi_env, val: napi::sys::napi_value) -> String {
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
    String::from_utf8_lossy(&buf[..actual]).to_string()
}

/// Coerce any JS value to a string (via `String(value)` semantics),
/// returning `"[object]"` if coercion itself fails.
unsafe fn coerce_js_to_string(env: napi::sys::napi_env, val: napi::sys::napi_value) -> String {
    let mut str_val = std::ptr::null_mut();
    let status = napi::sys::napi_coerce_to_string(env, val, &mut str_val);
    if status != napi::sys::Status::napi_ok {
        return "[object]".to_string();
    }
    read_js_string(env, str_val)
}

/// The message of a thrown JS value: an `Error`'s `message` when present,
/// otherwise the value coerced to a string (`throw "oops"`). Mirrors the
/// pyo3 side, which surfaces the real Python exception text.
unsafe fn extract_exception_message(
    env: napi::sys::napi_env,
    exception: napi::sys::napi_value,
) -> String {
    let mut key = std::ptr::null_mut();
    napi::sys::napi_create_string_utf8(
        env,
        "message".as_ptr() as *const _,
        "message".len() as isize,
        &mut key,
    );

    let mut has = false;
    napi::sys::napi_has_property(env, exception, key, &mut has);
    if has {
        let mut val = std::ptr::null_mut();
        napi::sys::napi_get_property(env, exception, key, &mut val);
        let mut vtype = 0i32;
        napi::sys::napi_typeof(env, val, &mut vtype);
        if vtype == 4 {
            return read_js_string(env, val);
        }
    }
    coerce_js_to_string(env, exception)
}

/// The bytes of a `Buffer` / `Uint8Array` / `Uint8ClampedArray`, or `None`
/// for any other JS value (including other TypedArray element types, whose
/// numeric interpretation would be lossy — they keep the string-coercion
/// fallback).
unsafe fn get_u8_array_bytes(
    env: napi::sys::napi_env,
    val: napi::sys::napi_value,
) -> Option<Vec<u8>> {
    let mut is_typedarray = false;
    napi::sys::napi_is_typedarray(env, val, &mut is_typedarray);
    if !is_typedarray {
        return None;
    }

    let mut ty: napi::sys::napi_typedarray_type = -1;
    let mut length = 0usize;
    let mut data = std::ptr::null_mut();
    let mut arraybuffer = std::ptr::null_mut();
    let mut byte_offset = 0usize;
    let status = napi::sys::napi_get_typedarray_info(
        env,
        val,
        &mut ty,
        &mut length,
        &mut data,
        &mut arraybuffer,
        &mut byte_offset,
    );
    if status != napi::sys::Status::napi_ok {
        return None;
    }
    if ty != napi::sys::TypedarrayType::uint8_array
        && ty != napi::sys::TypedarrayType::uint8_clamped_array
    {
        return None;
    }
    if length == 0 || data.is_null() {
        // A zero-length view (or a detached backing store) has no bytes to
        // copy; `data` may legitimately be null in that case.
        return Some(Vec::new());
    }
    // `data` already points at the first element (napi adjusts it by
    // `byte_offset`), and u8 elements are 1 byte, so `length` is the byte
    // count.
    Some(std::slice::from_raw_parts(data as *const u8, length).to_vec())
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

/// A row value crossing from Rust to JS. Mirrors [`dirsql::Value`] but
/// converts straight to napi values, so a BLOB surfaces as a Node `Buffer`.
pub enum JsRowValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl ToNapiValue for JsRowValue {
    unsafe fn to_napi_value(env: napi::sys::napi_env, val: Self) -> Result<napi::sys::napi_value> {
        match val {
            JsRowValue::Null => Null::to_napi_value(env, Null),
            JsRowValue::Integer(i) => i64::to_napi_value(env, i),
            JsRowValue::Real(f) => f64::to_napi_value(env, f),
            JsRowValue::Text(s) => String::to_napi_value(env, s),
            JsRowValue::Blob(b) => Buffer::to_napi_value(env, Buffer::from(b)),
        }
    }
}

/// `Number.MAX_SAFE_INTEGER` (2^53 - 1): the largest integer a JS `Number`
/// holds without precision loss.
const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// An `i64` a JS `Number` can represent exactly, or an error message naming
/// the value. Out-of-range integers must error rather than silently round
/// when they cross to JS.
fn ensure_js_safe_integer(i: i64) -> std::result::Result<i64, String> {
    if (-JS_MAX_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&i) {
        Ok(i)
    } else {
        Err(format!("integer {i} exceeds JS safe integer range"))
    }
}

fn value_to_js(v: &Value) -> Result<JsRowValue> {
    Ok(match v {
        Value::Null => JsRowValue::Null,
        Value::Integer(i) => {
            ensure_js_safe_integer(*i).map_err(to_napi_err)?;
            JsRowValue::Integer(*i)
        }
        Value::Real(f) => JsRowValue::Real(*f),
        Value::Text(s) => JsRowValue::Text(s.clone()),
        Value::Blob(b) => JsRowValue::Blob(b.clone()),
    })
}

fn value_row_to_js(row: &HashMap<String, Value>) -> Result<HashMap<String, JsRowValue>> {
    row.iter()
        .map(|(k, v)| Ok((k.clone(), value_to_js(v)?)))
        .collect()
}

fn row_event_to_js(event: &CoreRowEvent) -> Result<RowEvent> {
    Ok(match event {
        CoreRowEvent::Insert {
            table,
            row,
            file_path,
        } => RowEvent {
            table: Some(table.clone()),
            action: "insert".to_string(),
            row: Some(value_row_to_js(row)?),
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
            row: Some(value_row_to_js(new_row)?),
            old_row: Some(value_row_to_js(old_row)?),
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
            row: Some(value_row_to_js(row)?),
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_row() -> HashMap<String, Value> {
        HashMap::from([("k".to_string(), Value::Integer(7))])
    }

    #[test]
    fn value_to_js_maps_each_variant() {
        assert!(matches!(
            value_to_js(&Value::Null).unwrap(),
            JsRowValue::Null
        ));
        assert!(matches!(
            value_to_js(&Value::Integer(3)).unwrap(),
            JsRowValue::Integer(3)
        ));
        assert!(
            matches!(value_to_js(&Value::Real(1.5)).unwrap(), JsRowValue::Real(f) if (f - 1.5).abs() < f64::EPSILON)
        );
        assert!(
            matches!(value_to_js(&Value::Text("hi".into())).unwrap(), JsRowValue::Text(ref s) if s == "hi")
        );
        assert!(
            matches!(value_to_js(&Value::Blob(vec![1, 2])).unwrap(), JsRowValue::Blob(ref b) if b == &[1, 2])
        );
    }

    #[test]
    fn ensure_js_safe_integer_accepts_the_boundary() {
        assert_eq!(
            ensure_js_safe_integer(JS_MAX_SAFE_INTEGER),
            Ok(JS_MAX_SAFE_INTEGER)
        );
        assert_eq!(
            ensure_js_safe_integer(-JS_MAX_SAFE_INTEGER),
            Ok(-JS_MAX_SAFE_INTEGER)
        );
        assert_eq!(ensure_js_safe_integer(0), Ok(0));
    }

    #[test]
    fn ensure_js_safe_integer_rejects_out_of_range() {
        let err = ensure_js_safe_integer(JS_MAX_SAFE_INTEGER + 1).unwrap_err();
        assert!(err.contains(&(JS_MAX_SAFE_INTEGER + 1).to_string()));
        assert!(err.contains("safe integer"));
        assert!(ensure_js_safe_integer(-JS_MAX_SAFE_INTEGER - 1).is_err());
        assert!(ensure_js_safe_integer(i64::MAX).is_err());
    }

    #[test]
    fn value_to_js_errors_on_unsafe_integer() {
        assert!(value_to_js(&Value::Integer(JS_MAX_SAFE_INTEGER + 1)).is_err());
        assert!(value_to_js(&Value::Integer(JS_MAX_SAFE_INTEGER)).is_ok());
    }

    #[test]
    fn value_row_to_js_converts_every_entry() {
        let js = value_row_to_js(&one_row()).unwrap();
        assert!(matches!(js.get("k"), Some(JsRowValue::Integer(7))));
    }

    #[test]
    fn value_row_to_js_propagates_unsafe_integer() {
        let row = HashMap::from([("k".to_string(), Value::Integer(JS_MAX_SAFE_INTEGER + 1))]);
        assert!(value_row_to_js(&row).is_err());
    }

    #[test]
    fn row_event_to_js_insert() {
        let ev = CoreRowEvent::Insert {
            table: "t".into(),
            row: one_row(),
            file_path: "/f".into(),
        };
        let out = row_event_to_js(&ev).unwrap();
        assert_eq!(out.action, "insert");
        assert_eq!(out.table.as_deref(), Some("t"));
        assert!(out.row.is_some());
        assert!(out.old_row.is_none());
        assert!(out.error.is_none());
    }

    #[test]
    fn row_event_to_js_propagates_unsafe_integer() {
        let row = HashMap::from([("k".to_string(), Value::Integer(JS_MAX_SAFE_INTEGER + 1))]);
        let ev = CoreRowEvent::Insert {
            table: "t".into(),
            row,
            file_path: "/f".into(),
        };
        assert!(row_event_to_js(&ev).is_err());
    }

    #[test]
    fn row_event_to_js_update_carries_old_and_new() {
        let ev = CoreRowEvent::Update {
            table: "t".into(),
            old_row: one_row(),
            new_row: one_row(),
            file_path: "/f".into(),
        };
        let out = row_event_to_js(&ev).unwrap();
        assert_eq!(out.action, "update");
        assert!(out.row.is_some());
        assert!(out.old_row.is_some());
    }

    #[test]
    fn row_event_to_js_delete() {
        let ev = CoreRowEvent::Delete {
            table: "t".into(),
            row: one_row(),
            file_path: "/f".into(),
        };
        let out = row_event_to_js(&ev).unwrap();
        assert_eq!(out.action, "delete");
        assert!(out.old_row.is_none());
    }

    #[test]
    fn row_event_to_js_error_has_no_row_and_optional_table() {
        let ev = CoreRowEvent::Error {
            table: None,
            file_path: PathBuf::from("/f"),
            error: "boom".into(),
        };
        let out = row_event_to_js(&ev).unwrap();
        assert_eq!(out.action, "error");
        assert!(out.table.is_none());
        assert!(out.row.is_none());
        assert_eq!(out.error.as_deref(), Some("boom"));
    }

    #[test]
    fn to_napi_err_carries_message() {
        let err = to_napi_err("kaboom");
        assert!(err.reason.contains("kaboom"));
    }

    #[test]
    fn on_file_error_displays_inner() {
        assert_eq!(OnFileError("bad".to_string()).to_string(), "bad");
    }
}
