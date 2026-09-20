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

use dirsql::extension_resolution::{self, ConfigSource as CoreConfigSource, PlanEntry};
use dirsql::{
    DirSQL as CoreDirSQL, Extension, PreparedBuild, RawFileEvent, Row, RowEvent as CoreRowEvent,
    Table, Value, flatten_row_event,
};
use napi::Task;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Run the `dirsql` CLI in this process and return its exit code.
///
/// The npm launcher calls this instead of spawning a bundled binary, so the
/// addon is the only copy of the core a package ships (#739). `argv` excludes
/// the program name; the core prepends it for clap.
///
/// Returns rather than exiting — the caller (bin-shim's `mainInProcess`) owns
/// the process exit. Codes are ordinary status codes, never 130/143 (#737).
#[napi(js_name = "runCli")]
pub fn run_cli(argv: Vec<String>) -> i32 {
    let mut full = Vec::with_capacity(argv.len() + 1);
    full.push("dirsql".to_string());
    full.extend(argv);
    with_default_signal_disposition(|| dirsql::cli::run_cli(full))
}

const FATAL_SIGNALS: [i32; 2] = [libc::SIGINT, libc::SIGTERM];

/// Run `f` with SIGINT/SIGTERM at `SIG_DFL`, restoring the prior disposition
/// after.
///
/// The launcher's JS listeners cannot fire while this call blocks the event
/// loop, so without this the signal is absorbed and a scan becomes
/// SIGKILL-only. `server` still exits gracefully: the core registers its own
/// handler over `SIG_DFL` once it starts.
#[cfg(unix)]
fn with_default_signal_disposition<T>(f: impl FnOnce() -> T) -> T {
    #[expect(
        unsafe_code,
        reason = "no safe API reaches a signal disposition; the raw call is the whole point"
    )]
    let saved: Vec<_> = FATAL_SIGNALS
        .iter()
        .map(|&sig| (sig, unsafe { libc::signal(sig, libc::SIG_DFL) }))
        .collect();
    let out = f();
    for (sig, prev) in saved {
        #[expect(unsafe_code, reason = "restores what the call above saved")]
        unsafe {
            libc::signal(sig, prev);
        }
    }
    out
}

#[cfg(not(unix))]
fn with_default_signal_disposition<T>(f: impl FnOnce() -> T) -> T {
    f()
}

/// The config paths the npm launcher must inspect for package-name
/// extensions; see [`dirsql::launcher::config_paths_from_argv`].
#[napi(js_name = "configPathsFromArgv")]
pub fn config_paths_from_argv(argv: Vec<String>) -> Vec<String> {
    dirsql::launcher::config_paths_from_argv(&argv)
}

/// The loadable-file suffixes a Node host recognizes.
const NODE_SUFFIXES: &[&str] = &[".so", ".dylib", ".dll", ".node"];

/// The subset this platform actually builds, used when picking a package's
/// loadable out of everything installed beside it.
#[cfg(target_os = "macos")]
const PLATFORM_SUFFIXES: &[&str] = &[".dylib", ".node"];
#[cfg(target_os = "windows")]
const PLATFORM_SUFFIXES: &[&str] = &[".dll", ".node"];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const PLATFORM_SUFFIXES: &[&str] = &[".so", ".node"];

/// One config file for the extension planner: its absolute path, and its
/// contents or `null` when the launcher could not read it.
#[napi(object)]
pub struct ConfigSource {
    pub path: String,
    pub contents: Option<String>,
}

/// One planned `[[dirsql.extension]]` entry.
///
/// Exactly one of `path` and `package` is set: a `path` is ready to load, a
/// `package` must be located with `require.resolve` first — unless `shadow`
/// names an existing file, which takes precedence over the package.
#[napi(object, object_from_js = false)]
pub struct ExtensionPlanEntry {
    pub path: Option<String>,
    pub package: Option<String>,
    pub shadow: Option<String>,
    pub entrypoint: Option<String>,
}

fn plan_entry_to_js(entry: PlanEntry) -> ExtensionPlanEntry {
    match entry {
        PlanEntry::Literal { path, entrypoint } => ExtensionPlanEntry {
            path: Some(display(&path)),
            package: None,
            shadow: None,
            entrypoint,
        },
        PlanEntry::Package {
            name,
            shadow,
            entrypoint,
        } => ExtensionPlanEntry {
            path: None,
            package: Some(name),
            shadow: Some(display(&shadow)),
            entrypoint,
        },
    }
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Plan several configs' `[[dirsql.extension]]` entries.
///
/// `null` back means no entry names a package, so the core's own config
/// loading handles them all.
#[napi(js_name = "planConfigExtensions")]
pub fn plan_config_extensions(configs: Vec<ConfigSource>) -> Option<Vec<ExtensionPlanEntry>> {
    let sources: Vec<CoreConfigSource<'_>> = configs
        .iter()
        .map(|source| CoreConfigSource {
            path: Path::new(&source.path),
            contents: source.contents.as_deref(),
        })
        .collect();
    let plan = extension_resolution::plan_config_extensions(&sources, NODE_SUFFIXES)?;
    Some(plan.into_iter().map(plan_entry_to_js).collect())
}

/// Pick package `name`'s single loadable file out of `candidates`.
#[napi(js_name = "selectLoadable")]
pub fn select_loadable(name: String, dirs: Vec<String>, candidates: Vec<String>) -> Result<String> {
    selected_loadable(&name, dirs, candidates).map_err(Error::from_reason)
}

/// The JS-free half of [`select_loadable`], so it stays unit-testable.
fn selected_loadable(
    name: &str,
    dirs: Vec<String>,
    candidates: Vec<String>,
) -> std::result::Result<String, String> {
    let dirs: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    let candidates: Vec<PathBuf> = candidates.into_iter().map(PathBuf::from).collect();
    extension_resolution::select_loadable(name, &dirs, &candidates, PLATFORM_SUFFIXES)
        .map(|path| display(&path))
        .map_err(|err| err.to_string())
}

/// Plan one extension path given outside a config file.
///
/// `base` is the directory a relative path and the bare-name shadow probe
/// resolve against; `resolveRelative` makes a relative path-looking value
/// absolute against it (config semantics) rather than verbatim.
#[napi(js_name = "planExtensionPath")]
pub fn plan_extension_path(
    path: String,
    base: String,
    resolve_relative: bool,
) -> ExtensionPlanEntry {
    plan_entry_to_js(extension_resolution::plan_extension_path(
        &path,
        Path::new(&base),
        resolve_relative,
        NODE_SUFFIXES,
    ))
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

/// One file the initial scan could not index, with the hook's own error.
///
/// A scan failure is not a scan *error*: the other files are indexed and the
/// database is usable. This is how a caller learns the index is incomplete,
/// and which files are missing from it.
///
/// Output-only (`object_from_js = false`): JS never constructs one.
#[napi(object, object_from_js = false)]
pub struct ScanFailure {
    /// Path relative to the scan root.
    pub path: String,
    /// The hook's error, as it rendered it.
    pub message: String,
}

/// The main DirSQL class. Creates an ephemeral SQLite index over a directory.
#[napi(js_name = "DirSQL")]
pub struct DirSQL {
    inner: RefCell<Option<CoreDirSQL>>,
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
        tables: Option<Array<'_>>,
        ignore: Option<Vec<String>>,
        config: Option<Vec<String>>,
        persist: Option<bool>,
        persist_path: Option<String>,
        extensions: Option<Vec<ExtensionSpec>>,
        suppress_config_extensions: Option<bool>,
        no_ignore: Option<bool>,
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
            no_ignore: no_ignore.unwrap_or(false),
        }))
    }

    /// Execute a SQL query and return results as an array of objects.
    ///
    /// Runs on the libuv threadpool so queries don't block the JS event loop.
    /// Returns a `Promise` in JS.
    #[napi(ts_return_type = "Promise<Record<string, unknown>[]>")]
    pub fn query(&self, sql: String) -> AsyncTask<QueryTask> {
        let inner = self.inner.borrow().as_ref().cloned();
        AsyncTask::new(QueryTask { inner, sql })
    }

    /// Start the file watcher. Must be called before pollEvents.
    ///
    /// Runs on the libuv threadpool so the JS event loop stays responsive
    /// while the watcher is being initialized. Returns a `Promise` in JS.
    #[napi(js_name = "startWatcher", ts_return_type = "Promise<void>")]
    pub fn start_watcher(&self) -> AsyncTask<StartWatcherTask> {
        let inner = self.inner.borrow().as_ref().cloned();
        AsyncTask::new(StartWatcherTask { inner })
    }

    /// Poll for file events with a timeout (in milliseconds).
    /// Returns an array of RowEvent objects, possibly empty.
    ///
    /// Runs on the libuv threadpool so the JS event loop stays responsive
    /// for the duration of the poll timeout. Returns a `Promise` in JS.
    #[napi(js_name = "pollEvents", ts_return_type = "Promise<RowEvent[]>")]
    pub fn poll_events(&self, timeout_ms: u32) -> AsyncTask<PollEventsTask> {
        let inner = self.inner.borrow().as_ref().cloned();
        AsyncTask::new(PollEventsTask { inner, timeout_ms })
    }

    /// The files the initial scan could not index. Empty after a clean scan.
    ///
    /// Reads an in-memory list the scan already produced, so unlike the other
    /// methods here it needs no threadpool hop and returns synchronously; the
    /// public wrapper is what makes it a `Promise`, so that it can await the
    /// scan first.
    #[napi(js_name = "scanFailures")]
    pub fn scan_failures(&self) -> Vec<ScanFailure> {
        self.inner
            .borrow()
            .as_ref()
            .map(|db| {
                db.scan_failures()
                    .iter()
                    .map(|f| ScanFailure {
                        path: f.path.clone(),
                        message: f.message.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Explicitly close the database connection. Called for cleanup or to
    /// ensure the WAL checkpoint completes before reading the database with
    /// external tools (e.g. sql.js in tests). Returns `undefined`.
    #[napi]
    pub fn close(&self) {
        self.inner.borrow_mut().take();
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
    no_ignore: bool,
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
            .suppress_config_extensions(self.suppress_config_extensions)
            .no_ignore(self.no_ignore);
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
        Ok(DirSQL {
            inner: RefCell::new(Some(inner)),
        })
    }
}

/// Runs `DirSQL::query` on the libuv threadpool so the JS event loop stays
/// responsive. `CoreDirSQL` is cheap to clone (internally `Arc`-wrapped), so
/// each task owns its own handle for the lifetime of the query.
pub struct QueryTask {
    inner: Option<CoreDirSQL>,
    sql: String,
}

impl Task for QueryTask {
    type Output = Vec<HashMap<String, JsRowValue>>;
    type JsValue = Vec<HashMap<String, JsRowValue>>;

    fn compute(&mut self) -> Result<Self::Output> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| Error::new(Status::GenericFailure, "DirSQL instance closed"))?;
        let rows = inner.query(&self.sql).map_err(to_napi_err)?;
        rows.iter().map(value_row_to_js).collect()
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

/// Runs `DirSQL::start_watching` on the libuv threadpool. Idempotent on the
/// core side, so repeated calls from JS are still safe.
pub struct StartWatcherTask {
    inner: Option<CoreDirSQL>,
}

impl Task for StartWatcherTask {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> Result<Self::Output> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| Error::new(Status::GenericFailure, "DirSQL instance closed"))?;
        inner.start_watching().map_err(to_napi_err)
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
    inner: Option<CoreDirSQL>,
    timeout_ms: u32,
}

impl Task for PollEventsTask {
    type Output = Vec<RawFileEvent>;
    type JsValue = Vec<RowEvent>;

    fn compute(&mut self) -> Result<Self::Output> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| Error::new(Status::GenericFailure, "DirSQL instance closed"))?;
        inner
            .wait_file_events(Duration::from_millis(u64::from(self.timeout_ms)))
            .map_err(to_napi_err)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| Error::new(Status::GenericFailure, "DirSQL instance closed"))?;
        let row_events = inner.apply_file_events(output);
        row_events.iter().map(row_event_to_js).collect()
    }
}

/// Parse a JS array of `TableDef` objects into Rust [`Table`]s. Must run on
/// the JS thread: creates a persistent napi reference to each `onFile`
/// callback so it can be invoked later without a live JS call frame.
#[expect(unsafe_code, reason = "raw napi_sys reference creation")]
fn parse_tables_from_js(env: Env, tables: Array<'_>) -> Result<Vec<Table>> {
    (0..tables.len())
        .map(|i| {
            let def: Object<'_> = tables
                .get(i)?
                .ok_or_else(|| to_napi_err(format!("Missing table at index {i}")))?;
            let on_file = required::<Unknown<'_>>(&def, "onFile")?;
            if on_file.get_type()? != ValueType::Function {
                return Err(to_napi_err("Property 'onFile' must be a function"));
            }
            let strict = match def.get::<Unknown<'_>>("strict")? {
                Some(v) if v.get_type()? == ValueType::Boolean => bool::from_unknown(v)?,
                _ => false,
            };
            let fn_ref = unsafe { Arc::new(FnRef::new(env.raw(), on_file.raw())?) };
            let mut table = Table::try_new(
                required::<String>(&def, "name")?,
                required::<String>(&def, "ddl")?,
                required::<String>(&def, "glob")?,
                make_on_file_closure(fn_ref),
            );
            table.strict = strict;
            Ok(table)
        })
        .collect()
}

fn required<V: FromNapiValue>(def: &Object<'_>, name: &str) -> Result<V> {
    def.get(name)?
        .ok_or_else(|| to_napi_err(format!("Missing property: {name}")))
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

#[expect(
    unsafe_code,
    reason = "napi refs lack Send/Sync; all access stays on the JS thread"
)]
unsafe impl Send for FnRef {}
#[expect(
    unsafe_code,
    reason = "napi refs lack Send/Sync; all access stays on the JS thread"
)]
unsafe impl Sync for FnRef {}

impl FnRef {
    #[expect(unsafe_code, reason = "raw napi_sys reference creation")]
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

    #[expect(unsafe_code, reason = "raw napi_sys reference read")]
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

    #[expect(unsafe_code, reason = "raw napi_sys function invocation")]
    unsafe fn call_on_file(&self, abs_path: &str) -> Result<Vec<HashMap<String, Value>>> {
        let env = self.raw_env;
        let func = self.get_value()?;

        let mut js_path = std::ptr::null_mut();
        let status = napi::sys::napi_create_string_utf8(
            env,
            abs_path.as_ptr() as *const _,
            len_isize(abs_path),
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
                    exception_message(Unknown::from_raw_unchecked(env, exception))?,
                ));
            }
            return Err(Error::new(
                Status::GenericFailure,
                "on-file function call failed".to_string(),
            ));
        }

        rows_from_js(Unknown::from_raw_unchecked(env, result))
    }
}

impl Drop for FnRef {
    #[expect(unsafe_code, reason = "raw napi_sys reference deletion")]
    fn drop(&mut self) {
        unsafe {
            napi::sys::napi_delete_reference(self.raw_env, self.raw_ref);
        }
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[expect(unsafe_code, reason = "invokes the raw napi_sys onFile callback")]
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

/// A `&str` length as the `isize` the raw napi_sys string APIs take.
fn len_isize(s: &str) -> isize {
    isize::try_from(s.len()).expect("string length fits in isize")
}

fn rows_from_js(result: Unknown<'_>) -> Result<Vec<Row>> {
    let array =
        Array::from_unknown(result).map_err(|_| to_napi_err("on-file must return an array"))?;
    (0..array.len())
        .map(|i| {
            let row = match array.get::<Unknown<'_>>(i)? {
                Some(v) if v.get_type()? == ValueType::Object => Object::from_unknown(v)?,
                _ => return Err(to_napi_err("on-file must return an array of objects")),
            };
            Object::keys(&row)?
                .into_iter()
                .map(|key| {
                    let value = row
                        .get::<Unknown<'_>>(&key)?
                        .map_or(Ok(Value::Null), js_to_value)?;
                    Ok((key, value))
                })
                .collect::<Result<Row>>()
        })
        .collect()
}

fn js_to_value(val: Unknown<'_>) -> Result<Value> {
    match val.get_type()? {
        ValueType::Undefined | ValueType::Null => Ok(Value::Null),
        ValueType::Boolean => Ok(Value::Integer(i64::from(bool::from_unknown(val)?))),
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "the range guard plus Rust's saturating float-to-int cast keep the conversion \
                      defined; JS integers beyond 2^53 already lost precision in the double"
        )]
        ValueType::Number => {
            let n = f64::from_unknown(val)?;
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                Ok(Value::Integer(n as i64))
            } else {
                Ok(Value::Real(n))
            }
        }
        ValueType::String => Ok(Value::Text(String::from_unknown(val)?)),
        // BigInt: an INTEGER within i64, or an explicit range error. Never a
        // silent TEXT fallback (the lossy behavior this replaces).
        ValueType::BigInt => match BigInt::from_unknown(val)?.get_i64() {
            (i, true) => Ok(Value::Integer(i)),
            _ => Err(to_napi_err(format!(
                "BigInt {} exceeds the i64 range dirsql can store",
                coerce_to_string(val)?
            ))),
        },
        // `Buffer` / `Uint8Array` (Buffer is a Uint8Array subclass)
        // marshals to a BLOB; any other object shape falls through to
        // string coercion.
        _ => Ok(match u8_array_bytes(val)? {
            Some(bytes) => Value::Blob(bytes),
            None => Value::Text(coerce_to_string(val)?),
        }),
    }
}

/// `String(value)` semantics.
fn coerce_to_string(val: Unknown<'_>) -> Result<String> {
    val.coerce_to_string()?.into_utf8()?.into_owned()
}

/// The message of a thrown JS value: an `Error`'s `message` when present,
/// otherwise the value coerced to a string (`throw "oops"`). Mirrors the
/// pyo3 side, which surfaces the real Python exception text.
fn exception_message(exception: Unknown<'_>) -> Result<String> {
    if exception.get_type()? == ValueType::Object
        && let Some(message) = Object::from_unknown(exception)?.get::<Unknown<'_>>("message")?
        && message.get_type()? == ValueType::String
    {
        return String::from_unknown(message);
    }
    coerce_to_string(exception)
}

/// The bytes of a `Buffer` / `Uint8Array` / `Uint8ClampedArray`, or `None`
/// for any other JS value (including other TypedArray element types, whose
/// numeric interpretation would be lossy — they keep the string-coercion
/// fallback).
fn u8_array_bytes(val: Unknown<'_>) -> Result<Option<Vec<u8>>> {
    if !val.is_typedarray()? {
        return Ok(None);
    }
    let view = TypedArray::from_unknown(val)?;
    Ok(match view.typed_array_type {
        TypedArrayType::Uint8 | TypedArrayType::Uint8Clamped => Some(view.arraybuffer.to_vec()),
        _ => None,
    })
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
    #[expect(unsafe_code, reason = "`ToNapiValue` declares this method unsafe")]
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
    let flat = flatten_row_event(event);
    Ok(RowEvent {
        table: flat.table,
        action: flat.action.to_string(),
        row: flat.row.map(value_row_to_js).transpose()?,
        old_row: flat.old_row.map(value_row_to_js).transpose()?,
        error: flat.error,
        file_path: Some(flat.file_path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(path: &str, contents: Option<&str>) -> ConfigSource {
        ConfigSource {
            path: path.to_string(),
            contents: contents.map(str::to_string),
        }
    }

    #[test]
    fn plan_config_extensions_is_none_without_a_package_name() {
        assert!(
            plan_config_extensions(vec![config(
                "/cfg/.dirsql.toml",
                Some("[[dirsql.extension]]\npath = \"./vec0.so\"\n"),
            )])
            .is_none()
        );
    }

    #[test]
    fn plan_config_extensions_resolves_literals_against_their_config_dir() {
        let plan = plan_config_extensions(vec![config(
            "/cfg/.dirsql.toml",
            Some(
                "[[dirsql.extension]]\npath = \"./vec0.so\"\n\n[[dirsql.extension]]\npath = \"sqlite_vec\"\n",
            ),
        )])
        .expect("a package name forces a plan");
        assert_eq!(plan[0].path.as_deref(), Some("/cfg/./vec0.so"));
        assert_eq!(plan[1].package.as_deref(), Some("sqlite_vec"));
        assert_eq!(plan[1].shadow.as_deref(), Some("/cfg/sqlite_vec"));
    }

    #[test]
    fn plan_config_extensions_skips_an_unreadable_config() {
        assert!(plan_config_extensions(vec![config("/cfg/.dirsql.toml", None)]).is_none());
    }

    #[test]
    fn plan_config_extensions_carries_the_entrypoint() {
        let plan = plan_config_extensions(vec![config(
            "/cfg/.dirsql.toml",
            Some("[[dirsql.extension]]\npath = \"sqlite_vec\"\nentrypoint = \"init\"\n"),
        )])
        .expect("a package name forces a plan");
        assert_eq!(plan[0].entrypoint.as_deref(), Some("init"));
    }

    #[test]
    fn select_loadable_returns_the_single_match() {
        let found = selected_loadable(
            "vec",
            vec!["/pkg".into()],
            vec![
                "/pkg/README.md".into(),
                format!("/pkg/vec0{}", PLATFORM_SUFFIXES[0]),
            ],
        )
        .expect("one loadable file");
        assert_eq!(found, format!("/pkg/vec0{}", PLATFORM_SUFFIXES[0]));
    }

    #[test]
    fn select_loadable_carries_the_cores_message_verbatim() {
        let err =
            selected_loadable("vec", vec!["/pkg".into()], vec![]).expect_err("no loadable file");
        assert!(err.starts_with("no loadable extension file (*"), "{err}");
        assert!(
            err.ends_with("found in package 'vec' (searched /pkg)"),
            "{err}"
        );
    }

    #[test]
    fn a_programmatic_path_marshals_as_a_literal_entry() {
        let entry = plan_extension_path("ext/vec0.so".into(), "/cwd".into(), false);
        assert_eq!(entry.path.as_deref(), Some("ext/vec0.so"));
        assert_eq!(entry.package, None);
        let resolved = plan_extension_path("ext/vec0.so".into(), "/cwd".into(), true);
        assert_eq!(resolved.path.as_deref(), Some("/cwd/ext/vec0.so"));
    }

    #[test]
    fn a_programmatic_entry_applies_the_node_suffix_list() {
        let entry = plan_extension_path("vec0.pyd".into(), "/cwd".into(), false);
        assert_eq!(entry.package.as_deref(), Some("vec0.pyd"));
        assert_eq!(entry.shadow.as_deref(), Some("/cwd/vec0.pyd"));
        assert_eq!(
            plan_extension_path("vec0.node".into(), "/cwd".into(), false).package,
            None
        );
    }

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
    fn config_paths_from_argv_forwards_to_the_core_scan() {
        assert_eq!(
            config_paths_from_argv(vec!["-c".into(), "a.toml".into()]),
            ["a.toml"]
        );
    }

    #[test]
    fn to_napi_err_carries_message() {
        let err = to_napi_err("kaboom");
        assert!(err.reason.contains("kaboom"));
    }

    #[test]
    fn len_isize_is_the_byte_length() {
        assert_eq!(len_isize(""), 0);
        assert_eq!(len_isize("message"), 7);
        assert_eq!(len_isize("héllo"), 6);
    }

    #[test]
    fn on_file_error_displays_inner() {
        assert_eq!(OnFileError("bad".to_string()).to_string(), "bad");
    }
}
