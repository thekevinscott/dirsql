//! `dirsql` — an ephemeral SQL index over a local directory.
//!
//! The published crate surface is intentionally small: [`DirSQL`], [`AsyncDirSQL`],
//! [`Table`], [`Row`], [`RowEvent`], [`Value`], [`DirSqlError`]. Internal modules
//! (`config`, `db`, `differ`, `matcher`, `parser`, `scanner`, `watcher`) are
//! marked `#[doc(hidden)]`: they remain callable so in-crate benches and language
//! bindings in this workspace can reach them, but they are not part of the
//! stable public API.

/// Reusable command runner backing the command-backed events (#322).
pub mod command;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod db;
#[doc(hidden)]
pub mod differ;
#[doc(hidden)]
pub mod matcher;
#[doc(hidden)]
pub mod persist;
#[doc(hidden)]
pub mod scanner;
#[doc(hidden)]
pub mod watcher;

#[cfg(feature = "cli")]
pub mod cli;

use crate::command::Placeholder;
use crate::db::{Db, parse_table_name};
use crate::matcher::TableMatcher;
use crate::persist::{
    CachedFile, FileStat, build_meta, canonical_root, compute_glob_config_hash,
    create_sidecar_tables, delete_file as cache_delete_file, drop_user_tables, ensure_parent_dir,
    hash_file, meta_is_compatible, now_ns, read_cached_files, read_meta, read_rows_for_file,
    resolve_persist_path, upsert_file, write_meta,
};
use crate::scanner::scan_directory;
use crate::watcher::{FileEvent, Watcher};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use thiserror::Error;

pub use crate::config::ExtensionSpec as Extension;
pub use crate::db::{DbError, Value};
pub use crate::differ::RowEvent;
#[doc(hidden)]
pub use crate::watcher::FileEvent as RawFileEvent;

pub type Row = HashMap<String, Value>;
pub type WatchStream = UnboundedReceiver<RowEvent>;

type BoxError = Box<dyn StdError + Send + Sync + 'static>;
type ExtractFn = dyn Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static;

#[derive(Debug, Error)]
pub enum DirSqlError {
    #[error(transparent)]
    Core(#[from] DbError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("failed to lock shared state: {0}")]
    Lock(String),

    #[error("glob matcher error: {message}")]
    Matcher {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error("watch already started")]
    WatchAlreadyStarted,

    #[error("watcher error: {message}")]
    Watch {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error("table DDL could not be parsed: {0}")]
    Ddl(String),

    #[error("failed to load extension '{}': {source}", .path.display())]
    Extension {
        path: PathBuf,
        #[source]
        source: DbError,
    },

    #[error("duplicate table name: {0}")]
    DuplicateTable(String),

    #[error("extract error for {path}: {message}")]
    Extract { path: String, message: String },

    #[error("config error: {message}")]
    Config {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error(
        "query() only accepts read-only statements; SQLite classified this statement as a write"
    )]
    WriteForbidden,
}

impl DirSqlError {
    // Error-mapping helpers used with `?`/`map_err` (e.g.
    // `.map_err(DirSqlError::lock)`), factored out of inline `|e| ...`
    // closures so the conversion lives in one place. Their bodies run on
    // lock poisoning / SQLite failures and are exercised by the
    // poison/forced-error tests below.
    fn lock(e: impl std::fmt::Display) -> Self {
        DirSqlError::Lock(e.to_string())
    }

    /// Wrap a typed error in `Watch`, preserving the underlying error as a
    /// source so callers can `.source()` / downcast. Used by the
    /// `notify`-backed code paths.
    fn watch<E: StdError + Send + Sync + 'static>(e: E) -> Self {
        DirSqlError::Watch {
            message: e.to_string(),
            source: Some(Box::new(e)),
        }
    }

    /// Build a `Watch` error with only a message (no underlying source).
    /// Used by the mutually-exclusive-API guards and other internal
    /// invariants that aren't backed by a third-party error type.
    fn watch_msg(msg: impl Into<String>) -> Self {
        DirSqlError::Watch {
            message: msg.into(),
            source: None,
        }
    }

    fn config<E: StdError + Send + Sync + 'static>(e: E) -> Self {
        DirSqlError::Config {
            message: e.to_string(),
            source: Some(Box::new(e)),
        }
    }

    fn matcher<E: StdError + Send + Sync + 'static>(e: E) -> Self {
        DirSqlError::Matcher {
            message: e.to_string(),
            source: Some(Box::new(e)),
        }
    }

    fn sqlite(e: rusqlite::Error) -> Self {
        DirSqlError::Core(DbError::Sqlite(e))
    }
}

pub type Result<T> = std::result::Result<T, DirSqlError>;

/// A single table definition: DDL + glob + extract callback.
///
/// The `extract` callback receives the **absolute filesystem path** of each
/// matched file and returns the rows that file contributes. dirsql does not
/// read file contents itself; a callback that needs the file body reads it
/// inside the closure (`std::fs::read_to_string(path)` etc.). Callbacks that
/// derive columns purely from the path or from filesystem facts never touch
/// the file at all.
///
/// Use [`Table::new`] for infallible extractors or [`Table::try_new`] when the
/// extractor can itself fail (bad file content, IO errors inside the callback,
/// etc.). [`Table::strict`] rejects rows that don't match the DDL columns
/// exactly.
#[derive(Clone)]
pub struct Table {
    pub ddl: String,
    pub glob: String,
    pub strict: bool,
    extract: Arc<ExtractFn>,
}

impl Table {
    pub fn new<F>(ddl: impl Into<String>, glob: impl Into<String>, extract: F) -> Self
    where
        F: Fn(&str) -> Vec<Row> + Send + Sync + 'static,
    {
        Self::try_new(ddl, glob, move |path| {
            Ok::<Vec<Row>, BoxError>(extract(path))
        })
    }

    pub fn strict<F>(ddl: impl Into<String>, glob: impl Into<String>, extract: F) -> Self
    where
        F: Fn(&str) -> Vec<Row> + Send + Sync + 'static,
    {
        let mut table = Self::new(ddl, glob, extract);
        table.strict = true;
        table
    }

    pub fn try_new<F>(ddl: impl Into<String>, glob: impl Into<String>, extract: F) -> Self
    where
        F: Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static,
    {
        Self {
            ddl: ddl.into(),
            glob: glob.into(),
            extract: Arc::new(extract),
            strict: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct DirSqlInner {
    db: Mutex<Db>,
    root: PathBuf,
    /// Canonicalized form of `root`, used **only** for the live filesystem
    /// watcher. `notify` has surprising behavior when handed a relative path
    /// like `.` / `./data` (it may deliver no events at all, or deliver them
    /// under the cwd-joined path so the relative prefix no longer strips):
    /// the CLI binary works around this by canonicalizing its root before
    /// watching, and the SDK now does the same (#250). Derived once at
    /// construction via [`canonical_root`] (literal fallback when
    /// canonicalization fails, e.g. a not-yet-created root), so the user's
    /// `root` — and therefore the initial scan and the
    /// `_path` virtual column — stay byte-for-byte unchanged.
    watch_root: PathBuf,
    /// Pre-compiled matcher over all table globs plus ignore patterns.
    /// Built once at construction, reused by the initial scan and every
    /// subsequent watch iteration.
    matcher: TableMatcher,
    /// Table name -> extract closure, resolved once.
    extract_map: HashMap<String, Arc<ExtractFn>>,
    /// Table name -> strict flag, resolved once.
    strict_map: HashMap<String, bool>,
    /// Cached rows per file path for positional diffing on modify/delete.
    file_rows: Mutex<HashMap<String, (String, Vec<Row>)>>,
    /// Lazily-created filesystem watcher, shared by both the polling API
    /// ([`DirSQL::poll_events`]) and the channel-based API ([`DirSQL::watch`]).
    watcher: Mutex<Option<Watcher>>,
    /// `true` once [`DirSQL::poll_events`] has been called at least once.
    /// Locks out [`DirSQL::watch`] to prevent two consumers from draining
    /// the same underlying watcher.
    poll_used: AtomicBool,
    /// `true` once [`DirSQL::watch`] has spawned its background thread.
    /// Locks out [`DirSQL::poll_events`].
    watch_thread_started: AtomicBool,
    /// Poll interval used by the channel-based [`watch`](DirSQL::watch)
    /// loop. Bounds event-to-stream latency from above (and idle CPU from
    /// below). Defaults to 200ms — see [`DirSQLBuilder::poll_interval`].
    poll_interval: Duration,
    /// Filesystem seam used by the watch-upsert path. Always [`RealFs`] in
    /// production; unit tests inject a deterministic double via the
    /// `with_ignore_and_fs` test-seam constructor.
    fs: Arc<dyn FileSystem>,
}

#[derive(Clone)]
pub struct DirSQL {
    inner: Arc<DirSqlInner>,
}

impl DirSQL {
    /// Start building a `DirSQL`. See [`DirSQLBuilder`] for the available
    /// configuration methods. Call `.build()` to finish construction
    /// synchronously (or `.prepare()` + [`finish_build`](Self::finish_build)
    /// to split the scan across threads for async bindings).
    ///
    /// The builder is the single construction entrypoint. To load from a
    /// `.dirsql.toml`, pass the config path via `.config(path)`; to override
    /// the root, use `.root(path)`; to add tables programmatically, use
    /// `.table(t)` / `.tables(ts)`. When both a `.config()` and explicit
    /// `.root()` are set, the explicit root wins and a warning is emitted.
    pub fn builder() -> DirSQLBuilder {
        DirSQLBuilder::default()
    }

    /// Shortcut for `DirSQL::builder().root(root).tables(tables).build()`.
    pub fn new(root: impl Into<PathBuf>, tables: Vec<Table>) -> Result<Self> {
        DirSQL::builder().root(root).tables(tables).build()
    }

    /// Shortcut for `DirSQL::builder().root(...).tables(...).ignore(...).build()`.
    pub fn with_ignore<I, S>(
        root: impl Into<PathBuf>,
        tables: Vec<Table>,
        ignore: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        DirSQL::builder()
            .root(root)
            .tables(tables)
            .ignore(ignore)
            .build()
    }

    /// Shortcut for `DirSQL::builder().config(root/.dirsql.toml).build()`.
    pub fn from_config(root: impl Into<PathBuf>) -> Result<Self> {
        DirSQL::builder()
            .config(root.into().join(".dirsql.toml"))
            .build()
    }

    /// Shortcut for `DirSQL::builder().config(config_path).build()`.
    pub fn from_config_path(config_path: impl AsRef<Path>) -> Result<Self> {
        DirSQL::builder()
            .config(config_path.as_ref().to_path_buf())
            .build()
    }

    /// Run a SQL query against the in-memory database.
    ///
    /// Only read-only statements are accepted. Each statement is prepared on
    /// SQLite and then classified via `sqlite3_stmt_readonly`; anything that
    /// SQLite itself flags as a write — `INSERT`, `UPDATE`, `DELETE`, `DROP`,
    /// `CREATE`, `ALTER`, `REPLACE`, `VACUUM`, `ANALYZE`, etc. — is rejected
    /// with [`DirSqlError::WriteForbidden`] before any rows are produced. This
    /// keeps the in-memory index consistent with the on-disk files that back
    /// it: mutations only happen through the watcher/indexer pipeline.
    pub fn query(&self, sql: &str) -> Result<Vec<Row>> {
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        db.query(sql).map_err(map_db_error)
    }

    /// Lazily create the filesystem watcher. Idempotent; subsequent calls are
    /// no-ops. Called implicitly by [`poll_events`](Self::poll_events) and
    /// [`watch`](Self::watch).
    pub fn start_watching(&self) -> Result<()> {
        let mut guard = self.inner.watcher.lock().map_err(DirSqlError::lock)?;
        if guard.is_none() {
            // Watch the canonicalized root, never the (possibly relative)
            // user-supplied one — `notify` misbehaves on relative paths (#250).
            let watcher = Watcher::new(&self.inner.watch_root).map_err(DirSqlError::watch)?;
            *guard = Some(watcher);
        }
        Ok(())
    }

    /// Poll-based watch API. Blocks up to `timeout` waiting for the next
    /// filesystem event, then drains any additional events that arrived during
    /// processing, applying all of them to the in-memory database. Returns the
    /// batch of [`RowEvent`]s produced (possibly empty). Safe to call in a
    /// loop.
    ///
    /// Mutually exclusive with [`watch`](Self::watch): calling `watch` after
    /// `poll_events` (or vice versa) returns an error, because both would
    /// drain the same underlying filesystem watcher.
    pub fn poll_events(&self, timeout: Duration) -> Result<Vec<RowEvent>> {
        if self.inner.watch_thread_started.load(Ordering::SeqCst) {
            return Err(DirSqlError::watch_msg(
                "watch() is active; cannot mix with poll_events()",
            ));
        }
        self.inner.poll_used.store(true, Ordering::SeqCst);
        self.start_watching()?;
        self.poll_once(timeout)
    }

    /// Split-phase wait helper used by async bindings that cannot safely
    /// invoke the `extract` callback off the host thread (e.g. the napi-rs
    /// TypeScript binding, where JS callbacks must run on the main JS
    /// thread). Blocks up to `timeout` for raw file events and returns them
    /// unprocessed. Pair with [`apply_file_events`](Self::apply_file_events)
    /// to finish the pipeline on the correct thread.
    #[doc(hidden)]
    pub fn wait_file_events(&self, timeout: Duration) -> Result<Vec<FileEvent>> {
        if self.inner.watch_thread_started.load(Ordering::SeqCst) {
            return Err(DirSqlError::watch_msg(
                "watch() is active; cannot mix with poll_events()",
            ));
        }
        self.inner.poll_used.store(true, Ordering::SeqCst);
        self.start_watching()?;
        let guard = self.inner.watcher.lock().map_err(DirSqlError::lock)?;
        let watcher = guard
            .as_ref()
            .ok_or_else(|| DirSqlError::watch_msg("watcher not started"))?;
        let mut events = Vec::new();
        if let Some(first) = watcher.recv_timeout(timeout) {
            events.push(first);
            events.extend(watcher.try_recv_all());
        }
        Ok(events)
    }

    /// Apply a batch of raw file events through the extract/DB update
    /// pipeline. Counterpart to [`wait_file_events`](Self::wait_file_events).
    /// Runs the `extract` callback inline, so the caller must invoke this on
    /// a thread where that callback is safe to call (the JS main thread for
    /// the TypeScript binding).
    #[doc(hidden)]
    pub fn apply_file_events(&self, events: Vec<FileEvent>) -> Vec<RowEvent> {
        let mut out = Vec::new();
        for fe in events {
            out.extend(self.process_file_event(fe));
        }
        out
    }

    /// Channel-based watch API. Spawns a background thread that pushes
    /// [`RowEvent`]s into the returned stream. Intended for long-running Rust
    /// consumers (e.g. a CLI `watch` command). Can only be called once per
    /// `DirSQL` instance.
    ///
    /// Mutually exclusive with [`poll_events`](Self::poll_events).
    pub fn watch(&self) -> Result<WatchStream> {
        if self.inner.poll_used.load(Ordering::SeqCst) {
            return Err(DirSqlError::watch_msg(
                "poll_events() already in use; cannot call watch()",
            ));
        }
        if self.inner.watch_thread_started.swap(true, Ordering::SeqCst) {
            return Err(DirSqlError::WatchAlreadyStarted);
        }
        self.start_watching()?;

        let (tx, rx) = unbounded();
        let this = self.clone();
        thread::spawn(move || run_channel_loop(this, tx));
        Ok(rx)
    }

    // ----- internals --------------------------------------------------------

    /// One iteration of the watch loop: block up to `timeout` for events,
    /// drain any extras, process them into row events + DB mutations.
    fn poll_once(&self, timeout: Duration) -> Result<Vec<RowEvent>> {
        let file_events = {
            let guard = self.inner.watcher.lock().map_err(DirSqlError::lock)?;
            let watcher = guard
                .as_ref()
                .ok_or_else(|| DirSqlError::watch_msg("watcher not started"))?;
            let mut events = Vec::new();
            if let Some(first) = watcher.recv_timeout(timeout) {
                events.push(first);
                events.extend(watcher.try_recv_all());
            }
            events
        };

        let mut out = Vec::new();
        for fe in file_events {
            out.extend(self.process_file_event(fe));
        }
        Ok(out)
    }

    /// Process a single [`FileEvent`], mutating the DB and cache as needed.
    /// Operational errors become [`RowEvent::Error`] items in the returned
    /// vec (matching the semantics of the channel-based watch loop).
    fn process_file_event(&self, event: FileEvent) -> Vec<RowEvent> {
        let abs_path = match &event {
            FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => p.clone(),
        };
        // Events now arrive under the canonical `watch_root` (the watcher was
        // started on it), so strip that first; fall back to the user-supplied
        // `root` (covers the already-canonical/absolute-root case and any
        // event whose path predates the watch-root change), then to the raw
        // absolute path. This keeps the computed relative `_path` identical to
        // the pre-#250 behavior for both absolute and relative roots.
        let rel_path_buf = abs_path
            .strip_prefix(&self.inner.watch_root)
            .or_else(|_| abs_path.strip_prefix(&self.inner.root))
            .unwrap_or(&abs_path)
            .to_path_buf();

        if self.inner.matcher.is_ignored(&rel_path_buf) {
            return Vec::new();
        }

        let table_name = match self.inner.matcher.match_file(&rel_path_buf) {
            Some(name) => name.to_string(),
            None => return Vec::new(),
        };
        let rel_path = rel_path_buf.to_string_lossy().to_string();

        match event {
            FileEvent::Deleted(_) => self.handle_delete(&table_name, &rel_path),
            FileEvent::Created(_) | FileEvent::Modified(_) => {
                self.handle_upsert(&table_name, &abs_path, &rel_path)
            }
        }
    }

    fn handle_delete(&self, table: &str, rel_path: &str) -> Vec<RowEvent> {
        let old_rows = match self.inner.file_rows.lock() {
            Ok(mut file_rows) => file_rows.remove(rel_path).map(|(_, r)| r),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        let row_events = differ::diff(table, old_rows.as_deref(), None, rel_path);

        let delete_result = match self.inner.db.lock() {
            Ok(db) => db.delete_rows_by_file(table, rel_path),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        if let Err(e) = delete_result {
            return vec![error_event(Some(table), rel_path, e.to_string())];
        }

        row_events
    }

    fn handle_upsert(&self, table: &str, abs_path: &Path, rel_path: &str) -> Vec<RowEvent> {
        // The file may have vanished between the watcher event and now.
        match self.inner.fs.stat(abs_path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        }

        let extract = match self.inner.extract_map.get(table) {
            Some(e) => e,
            None => return Vec::new(),
        };

        let raw_rows = match extract(&abs_path.to_string_lossy()) {
            Ok(r) => r,
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        let captures = self
            .inner
            .matcher
            .match_file_with_captures(Path::new(rel_path))
            .map(|m| m.captures)
            .unwrap_or_default();
        let stat = compute_stat_virtuals(rel_path, abs_path);

        let strict = *self.inner.strict_map.get(table).unwrap_or(&false);

        let new_rows = {
            let db = match self.inner.db.lock() {
                Ok(g) => g,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
            let declared_columns = match db.get_table_columns(table) {
                Ok(cols) => cols,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
            let raw_rows = merge_filesystem_facts(raw_rows, &captures, &stat, &declared_columns);
            let mut normalized = Vec::with_capacity(raw_rows.len());
            for raw in &raw_rows {
                match db.normalize_row(table, raw, strict) {
                    Ok(row) => normalized.push(row),
                    Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
                }
            }
            normalized
        };

        let old_rows = match self.inner.file_rows.lock() {
            Ok(guard) => guard.get(rel_path).map(|(_, r)| r.clone()),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        let row_events = differ::diff(table, old_rows.as_deref(), Some(&new_rows), rel_path);

        let db_result = match self.inner.db.lock() {
            Ok(db) => db.delete_rows_by_file(table, rel_path).and_then(|_| {
                for (i, row) in new_rows.iter().enumerate() {
                    db.insert_row(table, row, rel_path, i)?;
                }
                Ok(())
            }),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        if let Err(e) = db_result {
            return vec![error_event(Some(table), rel_path, e.to_string())];
        }

        if let Ok(mut guard) = self.inner.file_rows.lock() {
            guard.insert(rel_path.to_string(), (table.to_string(), new_rows));
        }

        row_events
    }

    pub(crate) fn build_from_resolved(resolved: ResolvedBuild) -> Result<Self> {
        let prepared = Self::prepare_resolved(resolved)?;
        Self::finish_build(prepared)
    }

    /// Test-seam build path: identical to [`build_from_resolved`] but stores
    /// the supplied [`FileSystem`] double on the resulting instance so the
    /// watch-upsert path's filesystem read can be faked deterministically.
    /// The prepare phase still uses [`RealFs`] (it has no instance yet), but
    /// the unit tests that exercise this seam build over an empty temp dir, so
    /// the scan touches nothing.
    #[cfg(test)]
    pub(crate) fn with_ignore_and_fs<I, S>(
        root: impl Into<PathBuf>,
        tables: Vec<Table>,
        ignore: I,
        fs: Arc<dyn FileSystem>,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let resolved = ResolvedBuild {
            root: root.into(),
            tables,
            ignore: ignore.into_iter().map(Into::into).collect(),
            extensions: Vec::new(),
            persist: false,
            persist_path: None,
            poll_interval: DEFAULT_POLL_INTERVAL,
        };
        let prepared = Self::prepare_resolved(resolved)?;
        Self::finish_build_with_fs(prepared, fs)
    }

    /// Split-phase construction — part 1. Performs all I/O that is safe to run
    /// off the host's main thread: validates DDL, compiles the matcher, walks
    /// the directory, opens the persistent cache (when enabled) and decides
    /// which files need re-parsing. Does **not** read file contents and does
    /// **not** invoke `extract`.
    ///
    /// Pair with [`finish_build`](Self::finish_build) to complete construction
    /// on a thread where the `extract` callback can safely execute (e.g. the
    /// JS main thread for the napi-rs TypeScript binding).
    #[doc(hidden)]
    pub fn prepare_resolved(resolved: ResolvedBuild) -> Result<PreparedBuild> {
        let ResolvedBuild {
            root,
            tables,
            ignore,
            extensions,
            persist,
            persist_path,
            poll_interval,
        } = resolved;

        let (matcher, table_names) = compile_matcher(&tables, &ignore)?;

        // Resolve the persistent context first (when enabled), so that
        // file scanning can consult the cached file index.
        let persist_ctx = if persist {
            Some(prepare_persist(
                &root,
                &tables,
                &ignore,
                persist_path.as_deref(),
            )?)
        } else {
            None
        };

        // Walk the directory once.
        let scanned = scan_directory(&root, &matcher);

        // Build the list of files needing re-parse. When persist is
        // enabled, files whose stat tuple matches the cache (and that pass
        // the racy-window check) are trusted instead of re-parsed.
        let (scanned_files, trusted, deleted) = match &persist_ctx {
            None => {
                let mut files = Vec::with_capacity(scanned.len());
                for (path, table_name) in scanned {
                    files.push(ScannedFile {
                        rel_path: relative_path(&root, &path),
                        table_name,
                        stat: None,
                    });
                }
                (files, Vec::new(), Vec::new())
            }
            Some(ctx) => reconcile_scan(&root, scanned, ctx, &RealFs)?,
        };

        let _ = table_names;

        Ok(PreparedBuild {
            root,
            tables,
            extensions,
            matcher,
            scanned_files,
            persist: persist_ctx.map(|ctx| PreparedPersist {
                db: ctx.db,
                trusted,
                deleted,
                meta: ctx.expected_meta,
                cold_rebuild: ctx.cold_rebuild,
            }),
            poll_interval,
        })
    }

    /// Split-phase construction — part 2. Consumes the intermediate state from
    /// [`prepare_resolved`](Self::prepare_resolved): creates the SQLite
    /// database (or wires up the persistent on-disk one), runs each table's
    /// DDL, invokes each file's `extract` callback, and inserts the
    /// resulting rows.
    ///
    /// Must be invoked on a thread where the `extract` closures can safely
    /// run. For the napi-rs binding that is the JS main thread.
    #[doc(hidden)]
    pub fn finish_build(prepared: PreparedBuild) -> Result<Self> {
        Self::finish_build_with_fs(prepared, Arc::new(RealFs))
    }

    /// Test-seam variant of [`finish_build`] that takes the [`FileSystem`]
    /// double to store on the instance. Production always passes
    /// `Arc::new(RealFs)` (via [`finish_build`]); unit tests inject a fake so
    /// the watch-upsert path's `stat` read is deterministic.
    pub(crate) fn finish_build_with_fs(
        prepared: PreparedBuild,
        fs: Arc<dyn FileSystem>,
    ) -> Result<Self> {
        let PreparedBuild {
            root,
            tables,
            extensions,
            matcher,
            scanned_files,
            persist,
            poll_interval,
        } = prepared;

        let (db, persist_ready) = match persist {
            Some(p) => (p.db, Some((p.trusted, p.deleted, p.meta, p.cold_rebuild))),
            None => (Db::new()?, None),
        };

        // Load configured SQLite extensions onto the connection before any
        // CREATE TABLE so a table's DDL and later queries can use
        // extension-provided functions. (An extension-backed *virtual table*
        // cannot be a dirsql-managed `[[table]]` — those inject per-file
        // tracking columns; see Db::create_table.) Loading is enabled only for
        // the duration of each load and disabled again afterwards.
        for ext in &extensions {
            db.load_extension(&ext.path, ext.entrypoint.as_deref())
                .map_err(|source| DirSqlError::Extension {
                    path: ext.path.clone(),
                    source,
                })?;
        }

        let mut extract_map: HashMap<String, Arc<ExtractFn>> = HashMap::new();
        let mut strict_map: HashMap<String, bool> = HashMap::new();
        let mut ddl_map: HashMap<String, String> = HashMap::new();

        for table in tables {
            let table_name =
                parse_table_name(&table.ddl).ok_or_else(|| DirSqlError::Ddl(table.ddl.clone()))?;
            // When the cache already holds this table from a prior run,
            // skip CREATE TABLE: the schema is preserved verbatim across
            // runs (the glob_config_hash captures the DDL).
            if !table_exists(&db, &table_name)? {
                db.create_table(&table.ddl)?;
            }
            extract_map.insert(table_name.clone(), table.extract);
            strict_map.insert(table_name.clone(), table.strict);
            ddl_map.insert(table_name, table.ddl);
        }

        let mut file_rows: HashMap<String, (String, Vec<Row>)> = HashMap::new();

        // First, apply trusted-file rebuilds of the in-memory file_rows
        // cache from the on-disk SQLite. These files are NOT re-parsed.
        if let Some((trusted, deleted, _, _)) = persist_ready.as_ref() {
            for tf in trusted {
                let user_columns = db.get_table_columns(&tf.table_name).map_err(map_db_error)?;
                let rows =
                    read_rows_for_file(db.conn(), &tf.table_name, &tf.rel_path, &user_columns)
                        .map_err(DirSqlError::sqlite)?;
                file_rows.insert(tf.rel_path.clone(), (tf.table_name.clone(), rows));
            }

            for (rel_path, table_name) in deleted {
                db.delete_rows_by_file(table_name, rel_path)
                    .map_err(map_db_error)?;
                cache_delete_file(db.conn(), rel_path).map_err(DirSqlError::sqlite)?;
            }
        }

        // Process every file that needs (re)parsing.
        let snapshot_ns = now_ns();
        for ScannedFile {
            rel_path,
            table_name,
            stat,
        } in scanned_files
        {
            let extract = extract_map.get(&table_name).ok_or_else(|| {
                DirSqlError::Ddl(format!("missing extract function for table {table_name}"))
            })?;
            let strict = *strict_map.get(&table_name).unwrap_or(&false);
            let abs_path = root.join(&rel_path);
            let raw_rows =
                extract(&abs_path.to_string_lossy()).map_err(|e| DirSqlError::Extract {
                    path: rel_path.clone(),
                    message: e.to_string(),
                })?;

            let captures = matcher
                .match_file_with_captures(Path::new(&rel_path))
                .map(|m| m.captures)
                .unwrap_or_default();
            let stat_virtuals = compute_stat_virtuals(&rel_path, &abs_path);
            let declared_columns = db.get_table_columns(&table_name).map_err(map_db_error)?;
            let raw_rows =
                merge_filesystem_facts(raw_rows, &captures, &stat_virtuals, &declared_columns);

            let mut rows = Vec::with_capacity(raw_rows.len());
            // When updating an existing file in the persistent cache, drop
            // its old rows before inserting the new ones.
            if persist_ready.is_some() {
                db.delete_rows_by_file(&table_name, &rel_path)
                    .map_err(map_db_error)?;
            }
            for (row_index, raw_row) in raw_rows.iter().enumerate() {
                let row = db.normalize_row(&table_name, raw_row, strict)?;
                db.insert_row(&table_name, &row, &rel_path, row_index)?;
                rows.push(row);
            }

            // Update the persistent file index after a successful parse.
            if persist_ready.is_some()
                && let Some(stat) = stat.as_ref()
            {
                let hash = hash_file(&root.join(&rel_path)).ok();
                upsert_file(
                    db.conn(),
                    &rel_path,
                    &table_name,
                    stat,
                    hash.as_ref(),
                    snapshot_ns,
                )
                .map_err(DirSqlError::sqlite)?;
            }

            file_rows.insert(rel_path, (table_name, rows));
        }

        // Write the meta block last so a crash mid-build leaves an
        // incompatible cache that is detected on the next startup.
        if let Some((_, _, meta, _)) = persist_ready.as_ref() {
            write_meta(db.conn(), meta).map_err(DirSqlError::sqlite)?;
        }

        // Canonicalize the watch root once, here at the single shared
        // construction point reached by both `build()` and `build_async()`,
        // so the live watcher never sees a relative path (#250). `root` itself
        // is left untouched.
        let watch_root = PathBuf::from(fs.canonical_root(&root));

        Ok(Self {
            inner: Arc::new(DirSqlInner {
                db: Mutex::new(db),
                root,
                watch_root,
                matcher,
                extract_map,
                strict_map,
                file_rows: Mutex::new(file_rows),
                watcher: Mutex::new(None),
                poll_used: AtomicBool::new(false),
                watch_thread_started: AtomicBool::new(false),
                poll_interval,
                fs,
            }),
        })
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`DirSQL`] and [`AsyncDirSQL`].
///
/// All configuration methods return `self` and are chainable. Call
/// [`build`](Self::build) for synchronous construction, or
/// [`build_async`](Self::build_async) to produce an [`AsyncDirSQL`] whose
/// initial scan runs on a background thread.
///
/// # Example
/// ```ignore
/// use dirsql::{DirSQL, Table};
/// let db = DirSQL::builder()
///     .root("./data")
///     .table(Table::new("CREATE TABLE t (x TEXT)", "*.json", |_, _| vec![]))
///     .ignore(["target/**"])
///     .build()?;
/// ```
///
/// # Config files
/// Pass a `.dirsql.toml` path via [`config`](Self::config). If the config
/// file declares a `root` field, it is resolved relative to the config's
/// parent directory. If both the config and an explicit [`root`](Self::root)
/// are provided, the explicit root wins and a warning is emitted.
#[derive(Default)]
pub struct DirSQLBuilder {
    root: Option<PathBuf>,
    tables: Vec<Table>,
    ignore: Vec<String>,
    extensions: Vec<Extension>,
    config_path: Option<PathBuf>,
    suppress_config_extensions: bool,
    persist: bool,
    persist_path: Option<PathBuf>,
    poll_interval: Option<Duration>,
}

impl DirSQLBuilder {
    /// Set the root directory to scan. Overrides any `root` declared by a
    /// config file passed via [`config`](Self::config), emitting a warning
    /// on stderr to flag the collision.
    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = Some(root.into());
        self
    }

    /// Replace the accumulated table list with `tables`.
    pub fn tables(mut self, tables: Vec<Table>) -> Self {
        self.tables = tables;
        self
    }

    /// Append a single table to the table list.
    pub fn table(mut self, table: Table) -> Self {
        self.tables.push(table);
        self
    }

    /// Replace the accumulated ignore-pattern list with `ignore`.
    pub fn ignore<I, S>(mut self, ignore: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignore = ignore.into_iter().map(Into::into).collect();
        self
    }

    /// Append a single SQLite extension to load at startup. Extensions are
    /// loaded onto the connection before any `CREATE TABLE`, then loading is
    /// disabled again. See [`Extension`].
    ///
    /// A relative `path` here is used verbatim — the OS resolves it against the
    /// process working directory at load time. Config-file paths, by contrast,
    /// resolve against the config file's parent directory.
    pub fn extension(mut self, extension: Extension) -> Self {
        self.extensions.push(extension);
        self
    }

    /// Replace the accumulated extension list with `extensions`.
    pub fn extensions<I>(mut self, extensions: I) -> Self
    where
        I: IntoIterator<Item = Extension>,
    {
        self.extensions = extensions.into_iter().collect();
        self
    }

    /// Load a `.dirsql.toml` config file at build time. The file's `[[table]]`
    /// entries are appended after any programmatic tables; its `[dirsql].ignore`
    /// patterns are appended; its optional `[dirsql].root` is resolved relative
    /// to the config's parent directory. If the builder's own [`root`](Self::root)
    /// was also set, the explicit value wins (with a warning).
    pub fn config(mut self, config_path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(config_path.into());
        self
    }

    /// Suppress loading of a config file's `[[dirsql.extension]]` entries.
    ///
    /// The core resolves config-file extension paths only literally (relative
    /// to the config's parent). A launcher that resolves extensions itself —
    /// e.g. by **package name**, which needs an interpreter the compiled core
    /// lacks (Python `importlib`, Node `require.resolve`; see #227) — sets this
    /// and supplies the already-resolved literal paths via
    /// [`extensions`](Self::extensions) instead, so the config's own extension
    /// entries are not loaded a second time.
    pub fn suppress_config_extensions(mut self, suppress: bool) -> Self {
        self.suppress_config_extensions = suppress;
        self
    }

    /// Enable persistent on-disk storage. When `true`, the SQLite database is
    /// written to `<root>/.dirsql/cache.db` (override via
    /// [`persist_path`](Self::persist_path)) so subsequent startups only
    /// re-parse files that have actually changed. See
    /// `docs/guide/persistence.md` for the reconcile contract.
    pub fn persist(mut self, persist: bool) -> Self {
        self.persist = persist;
        self
    }

    /// Override the location of the persistent cache file. Ignored when
    /// [`persist`](Self::persist) is `false`.
    pub fn persist_path(mut self, path: impl AsRef<Path>) -> Self {
        self.persist_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set the poll interval used by the channel-based
    /// [`watch`](DirSQL::watch) loop. Bounds event-to-stream latency from
    /// above (low values: tighter latency, higher idle CPU) and from below
    /// (high values: lower idle CPU, slower reaction). Defaults to 200ms
    /// when not set.
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = Some(interval);
        self
    }

    /// Resolve all inputs (reading the config file if one was supplied) into
    /// a [`ResolvedBuild`] used by the construction pipeline. Emits a warning
    /// on stderr if both an explicit root and a config-supplied root are
    /// present.
    fn resolve(self) -> Result<ResolvedBuild> {
        let DirSQLBuilder {
            root: explicit_root,
            mut tables,
            mut ignore,
            mut extensions,
            config_path,
            suppress_config_extensions,
            mut persist,
            mut persist_path,
            poll_interval,
        } = self;

        let mut config_root: Option<PathBuf> = None;

        if let Some(ref cfg_path) = config_path {
            let cfg = config::load_config(cfg_path).map_err(DirSqlError::config)?;

            let cfg_parent = cfg_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let resolved_root = if let Some(cfg_root) = cfg.root.clone() {
                if cfg_root.is_absolute() {
                    cfg_root
                } else {
                    cfg_parent.join(cfg_root)
                }
            } else {
                cfg_parent.clone()
            };
            config_root = Some(resolved_root.clone());

            // `on-file` commands run in the config file's directory and compute
            // `{path}` relative to the resolved index root.
            let cfg_tables = build_tables_from_config(&cfg, &cfg_parent, &resolved_root)?;
            tables.extend(cfg_tables);
            ignore.extend(cfg.ignore);

            // Resolve config-supplied extension paths against the config
            // file's parent directory (absolute paths pass through). Appended
            // after any programmatically-supplied extensions. Skipped entirely
            // when the caller has pre-resolved the config's extensions itself
            // (e.g. a launcher resolving package names) and supplied them via
            // `.extensions(...)` — see `suppress_config_extensions`.
            if !suppress_config_extensions {
                for ext in cfg.extensions {
                    let path = if ext.path.is_absolute() {
                        ext.path
                    } else {
                        cfg_parent.join(&ext.path)
                    };
                    extensions.push(Extension {
                        path,
                        entrypoint: ext.entrypoint,
                    });
                }
            }

            if cfg.persist {
                persist = true;
            }
            if persist_path.is_none()
                && let Some(p) = cfg.persist_path.clone()
            {
                let resolved = if p.is_absolute() {
                    p
                } else {
                    cfg_parent.join(p)
                };
                persist_path = Some(resolved);
            }
        }

        let root = match (explicit_root, config_root) {
            (Some(explicit), Some(cfg)) => {
                if explicit != cfg {
                    eprintln!(
                        "dirsql: explicit .root({}) overrides config root ({})",
                        explicit.display(),
                        cfg.display(),
                    );
                }
                explicit
            }
            (Some(explicit), None) => explicit,
            (None, Some(cfg)) => cfg,
            (None, None) => {
                return Err(DirSqlError::Config {
                    message: "no root directory: call .root(...) or .config(path)".into(),
                    source: None,
                });
            }
        };

        Ok(ResolvedBuild {
            root,
            tables,
            ignore,
            extensions,
            persist,
            persist_path,
            poll_interval: poll_interval.unwrap_or(DEFAULT_POLL_INTERVAL),
        })
    }

    /// Finish building synchronously. Blocks on the initial directory scan.
    pub fn build(self) -> Result<DirSQL> {
        let resolved = self.resolve()?;
        DirSQL::build_from_resolved(resolved)
    }

    /// Split-phase prepare: runs the Send-safe portion of construction and
    /// returns the intermediate [`PreparedBuild`]. Intended for async bindings
    /// (napi, py-ext) that must finish on a specific thread.
    #[doc(hidden)]
    pub fn prepare(self) -> Result<PreparedBuild> {
        let resolved = self.resolve()?;
        DirSQL::prepare_resolved(resolved)
    }

    /// Finish building asynchronously. Returns immediately; the initial scan
    /// runs on a background thread. Call [`AsyncDirSQL::ready`] to await it.
    pub fn build_async(self) -> Result<AsyncDirSQL> {
        let resolved = self.resolve()?;
        Ok(AsyncDirSQL::spawn_build(resolved))
    }
}

/// Default poll interval for the channel-based watch loop. Used when the
/// builder doesn't supply an explicit `poll_interval`. Bounds event-to-
/// stream latency from above; lower values trade idle CPU for tighter
/// reaction time.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Fully-resolved builder inputs: the result of merging programmatic
/// settings with values loaded from a `.dirsql.toml` config file.
#[doc(hidden)]
pub struct ResolvedBuild {
    pub root: PathBuf,
    pub tables: Vec<Table>,
    pub ignore: Vec<String>,
    pub extensions: Vec<Extension>,
    pub persist: bool,
    pub persist_path: Option<PathBuf>,
    pub poll_interval: Duration,
}

/// A single file discovered during [`DirSQL::prepare_resolved`]: its
/// root-relative path, the table it belongs to, and the filesystem stat
/// tuple captured during the scan (when persist is on).
#[doc(hidden)]
pub struct ScannedFile {
    pub rel_path: String,
    pub table_name: String,
    pub stat: Option<FileStat>,
}

/// Intermediate state produced by [`DirSQL::prepare_resolved`] and consumed
/// by [`DirSQL::finish_build`]. Opaque on purpose; fields are only visible
/// to the in-workspace bindings.
#[doc(hidden)]
pub struct PreparedBuild {
    root: PathBuf,
    tables: Vec<Table>,
    /// SQLite extensions to load onto the connection before any table DDL.
    extensions: Vec<Extension>,
    matcher: TableMatcher,
    scanned_files: Vec<ScannedFile>,
    persist: Option<PreparedPersist>,
    /// Poll interval for the channel-based watch loop. Sourced from
    /// [`DirSQLBuilder::poll_interval`] or [`DEFAULT_POLL_INTERVAL`].
    poll_interval: Duration,
}

#[doc(hidden)]
pub struct PreparedPersist {
    db: Db,
    trusted: Vec<TrustedFile>,
    deleted: Vec<(String, String)>,
    meta: HashMap<String, String>,
    cold_rebuild: bool,
}

#[doc(hidden)]
pub struct TrustedFile {
    pub rel_path: String,
    pub table_name: String,
}

/// Internal context produced by [`prepare_persist`].
struct PersistContext {
    db: Db,
    cached: HashMap<String, CachedFile>,
    expected_meta: HashMap<String, String>,
    cold_rebuild: bool,
}

fn compile_matcher(
    tables: &[Table],
    ignore_patterns: &[String],
) -> Result<(TableMatcher, Vec<String>)> {
    let mut seen: HashMap<String, ()> = HashMap::with_capacity(tables.len());
    let mut mappings: Vec<(String, String)> = Vec::with_capacity(tables.len());
    let mut names = Vec::with_capacity(tables.len());
    for table in tables {
        let table_name =
            parse_table_name(&table.ddl).ok_or_else(|| DirSqlError::Ddl(table.ddl.clone()))?;
        // Validate up front so a poisoned name from a stored cache or a
        // would-be-injection DDL can't propagate into `extract_map`,
        // `strict_map`, or any format!()-built SQL down the line.
        crate::db::validate_identifier(&table_name).map_err(map_db_error)?;
        if seen.insert(table_name.clone(), ()).is_some() {
            return Err(DirSqlError::DuplicateTable(table_name));
        }
        mappings.push((table.glob.clone(), table_name.clone()));
        names.push(table_name);
    }

    let mapping_refs: Vec<(&str, &str)> = mappings
        .iter()
        .map(|(g, n)| (g.as_str(), n.as_str()))
        .collect();
    let ignore_refs: Vec<&str> = ignore_patterns.iter().map(String::as_str).collect();
    let matcher = TableMatcher::new(&mapping_refs, &ignore_refs).map_err(DirSqlError::matcher)?;
    Ok((matcher, names))
}

/// Open (or create) the persistent SQLite cache and read its meta. If the
/// meta is missing or incompatible with the current build, the cache is
/// wiped and the resulting [`PersistContext`] reports `cold_rebuild = true`
/// so the rest of the pipeline knows to treat every file as new.
fn prepare_persist(
    root: &Path,
    tables: &[Table],
    ignore: &[String],
    persist_path_override: Option<&Path>,
) -> Result<PersistContext> {
    let path = resolve_persist_path(root, persist_path_override);
    ensure_parent_dir(&path)?;

    let db = Db::open(&path).map_err(map_db_error)?;
    create_sidecar_tables(db.conn()).map_err(DirSqlError::sqlite)?;

    let glob_hash = compute_glob_config_hash(tables, ignore);
    let canonical = canonical_root(root);
    let expected_meta = build_meta(&glob_hash, &canonical);

    let cached_meta = read_meta(db.conn()).map_err(DirSqlError::sqlite)?;
    let compatible = !cached_meta.is_empty() && meta_is_compatible(&cached_meta, &expected_meta);

    let (cached, cold_rebuild) = if compatible {
        let files = read_cached_files(db.conn()).map_err(DirSqlError::sqlite)?;
        (files, false)
    } else {
        drop_user_tables(db.conn()).map_err(DirSqlError::sqlite)?;
        (HashMap::new(), true)
    };

    Ok(PersistContext {
        db,
        cached,
        expected_meta,
        cold_rebuild,
    })
}

/// Internal filesystem seam. Every effectful filesystem read performed by the
/// persist/reconcile and watch-upsert paths goes through this trait so unit
/// tests can inject a deterministic double (avoiding real `std::fs` calls and
/// the racy timing windows they imply). Production always uses [`RealFs`],
/// which replicates the previous inline `std::fs`/`hash_file` calls exactly --
/// this is purely a test seam, not a behavioral change.
trait FileSystem: Send + Sync {
    /// Stat a path. Mirrors `std::fs::metadata(path).map(|m| FileStat::from_metadata(&m))`.
    fn stat(&self, path: &Path) -> std::io::Result<FileStat>;
    /// BLAKE3-hash a file's contents. Mirrors [`hash_file`].
    fn hash(&self, path: &Path) -> std::io::Result<[u8; 32]>;
    /// Canonicalize the watch root (literal fallback). Mirrors
    /// [`canonical_root`](persist::canonical_root).
    fn canonical_root(&self, root: &Path) -> String;
}

/// Production [`FileSystem`]: delegates to the real `std::fs` / [`hash_file`]
/// calls that the persist and watch paths used inline before the seam existed.
struct RealFs;

impl FileSystem for RealFs {
    fn stat(&self, path: &Path) -> std::io::Result<FileStat> {
        std::fs::metadata(path).map(|m| FileStat::from_metadata(&m))
    }

    fn hash(&self, path: &Path) -> std::io::Result<[u8; 32]> {
        hash_file(path)
    }

    fn canonical_root(&self, root: &Path) -> String {
        canonical_root(root)
    }
}

/// Decide which files are trusted, which need re-parsing, and which were
/// removed since the last cache write.
#[allow(clippy::type_complexity)]
fn reconcile_scan(
    root: &Path,
    scanned: Vec<(PathBuf, String)>,
    ctx: &PersistContext,
    fs: &dyn FileSystem,
) -> Result<(Vec<ScannedFile>, Vec<TrustedFile>, Vec<(String, String)>)> {
    let mut to_parse = Vec::new();
    let mut trusted = Vec::new();
    let mut seen_paths: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(scanned.len());

    for (path, table_name) in scanned {
        let rel_path = relative_path(root, &path);
        seen_paths.insert(rel_path.clone());

        let stat = fs.stat(&path)?;

        let cached = ctx.cached.get(&rel_path);
        let trust = match cached {
            Some(c) if c.table_name == table_name && c.stat == stat => {
                // Stat matches. Outside the racy window? Trust the cache.
                if c.snapshot_ns > stat.mtime_ns {
                    true
                } else {
                    // Hash-confirm.
                    match (fs.hash(&path).ok(), c.content_hash) {
                        (Some(live), Some(cached_hash)) => live == cached_hash,
                        _ => false,
                    }
                }
            }
            _ => false,
        };

        if trust {
            trusted.push(TrustedFile {
                rel_path,
                table_name,
            });
        } else {
            to_parse.push(ScannedFile {
                rel_path,
                table_name,
                stat: Some(stat),
            });
        }
    }

    let mut deleted = Vec::new();
    for (rel_path, cf) in &ctx.cached {
        if !seen_paths.contains(rel_path) {
            deleted.push((rel_path.clone(), cf.table_name.clone()));
        }
    }

    Ok((to_parse, trusted, deleted))
}

fn table_exists(db: &Db, name: &str) -> Result<bool> {
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .map_err(DirSqlError::sqlite)?;
    Ok(count > 0)
}

/// Translate a [`DbError`] into a [`DirSqlError`], promoting the core's
/// structural write-rejection ([`DbError::WriteForbidden`]) into
/// [`DirSqlError::WriteForbidden`] so callers can distinguish a rejected
/// write from any other query error. Every other `DbError` flows through the
/// usual [`DirSqlError::Core`] conversion.
fn map_db_error(e: DbError) -> DirSqlError {
    match e {
        DbError::WriteForbidden => DirSqlError::WriteForbidden,
        other => DirSqlError::Core(other),
    }
}

fn error_event(table: Option<&str>, rel_path: &str, error: String) -> RowEvent {
    RowEvent::Error {
        table: table.map(str::to_string),
        file_path: PathBuf::from(rel_path),
        error,
    }
}

fn run_channel_loop(db: DirSQL, tx: UnboundedSender<RowEvent>) {
    let interval = db.inner.poll_interval;
    loop {
        match db.poll_once(interval) {
            Ok(events) => {
                for event in events {
                    if tx.unbounded_send(event).is_err() {
                        return;
                    }
                }
            }
            Err(e) => {
                let _ = tx.unbounded_send(RowEvent::Error {
                    table: None,
                    file_path: db.inner.root.clone(),
                    error: e.to_string(),
                });
                return;
            }
        }
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Fixed timeout for an `on-file` command. There is no per-table timeout key
/// yet (#327); this module constant is the documented current default.
const ON_FILE_TIMEOUT: Duration = Duration::from_secs(30);

/// Build [`Table`] objects from a parsed config.
///
/// A plain config-defined table produces one row per matched file built
/// entirely from filesystem facts: glob path captures and stat virtuals
/// (`_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`) are
/// injected by the core pipeline ([`merge_filesystem_facts`]). Its synthesized
/// extract emits a single empty row per file; the fact-injection layer fills it
/// in.
///
/// A table with an `on-file` command instead runs that command once per matched
/// file (see [`run_on_file`]): the command reads the file and prints a JSON
/// array of row objects on stdout, which becomes the file's rows (filesystem
/// facts are still merged on top, user values winning). `config_dir` is the
/// command's working directory (the config file's parent) and `root` is the
/// resolved index root used to compute the `{path}` placeholder.
fn build_tables_from_config(
    cfg: &config::Config,
    config_dir: &Path,
    root: &Path,
) -> Result<Vec<Table>> {
    let mut tables = Vec::with_capacity(cfg.tables.len());

    for table_cfg in &cfg.tables {
        let mut table = match &table_cfg.on_file {
            Some(command) => {
                let command = command.clone();
                let config_dir = config_dir.to_path_buf();
                let root = root.to_path_buf();
                // `Table::new` (infallible): `run_on_file` isolates its own
                // errors to an empty row set so one bad file never aborts the
                // scan (the scan aborts on an extract `Err`).
                Table::new(
                    table_cfg.ddl.clone(),
                    table_cfg.glob.clone(),
                    move |abs_path: &str| run_on_file(&command, abs_path, &config_dir, &root),
                )
            }
            None => Table::new(
                table_cfg.ddl.clone(),
                table_cfg.glob.clone(),
                |_path: &str| vec![Row::new()],
            ),
        };

        if table_cfg.strict == Some(true) {
            table.strict = true;
        }

        tables.push(table);
    }

    Ok(tables)
}

/// Run a table's `on-file` command for one matched file and parse its output
/// into rows.
///
/// Placeholders: `{path}` (the file relative to `root`, append-if-absent so
/// `cmd` and `cmd {path}` behave identically), `{abspath}` (the absolute path),
/// and `{root}` (the index root). The relative path is computed with a single
/// [`Path::strip_prefix`] (#251/#252), falling back to the absolute path when
/// the file is not under `root`.
///
/// Per-file isolation: any failure — a spawn/exit/timeout error from
/// [`command::run_command`], or output that is not a JSON array of objects —
/// is logged to stderr and yields no rows (`vec![]`). Returning `Err` here
/// would abort the whole scan, so it never does.
fn run_on_file(command: &str, abs_path: &str, config_dir: &Path, root: &Path) -> Vec<Row> {
    let abs = Path::new(abs_path);
    let rel = abs
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| abs_path.to_string());
    let placeholders = [
        Placeholder::append("path", rel),
        Placeholder::new("abspath", abs_path),
        Placeholder::new("root", root.to_string_lossy().into_owned()),
    ];

    match command::run_command(command, &placeholders, config_dir, ON_FILE_TIMEOUT, None) {
        Ok(output) => match parse_command_rows(&output.payload) {
            Ok(rows) => rows,
            Err(message) => {
                eprintln!(
                    "dirsql: skipping `{abs_path}`: on-file output was not a JSON array of rows: {message}"
                );
                Vec::new()
            }
        },
        Err(error) => {
            eprintln!("dirsql: skipping `{abs_path}`: on-file command failed: {error}");
            Vec::new()
        }
    }
}

/// Parse an `on-file` command's stdout payload — a JSON array of row objects —
/// into [`Row`]s. Returns `Err(msg)` when the top-level JSON is not an array or
/// any element is not an object. Pure (no IO), so it stays colocated-unit-
/// testable; the effectful spawn lives in [`run_on_file`].
fn parse_command_rows(payload: &str) -> std::result::Result<Vec<Row>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| format!("invalid JSON: {e}"))?;
    let array = parsed
        .as_array()
        .ok_or_else(|| "expected a JSON array of row objects".to_string())?;

    let mut rows = Vec::with_capacity(array.len());
    for element in array {
        let object = element
            .as_object()
            .ok_or_else(|| "expected each array element to be a JSON object".to_string())?;
        let mut row = Row::with_capacity(object.len());
        for (key, value) in object {
            row.insert(key.clone(), json_to_value(value));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Map a JSON value to a SQLite [`Value`]: `null` → `Null`; `bool` → `Integer`
/// (0/1); an integral number → `Integer`, otherwise `Real`; `string` → `Text`;
/// an array/object → its JSON text as `Text`. Pure.
fn json_to_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Integer(i64::from(*b)),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Real(n.as_f64().unwrap_or(f64::NAN)),
        },
        serde_json::Value::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    }
}

/// Reserved column names for filesystem-derived virtual columns. These are
/// always available on every row when declared in the table DDL; if not
/// declared, they are silently dropped during normalization.
const STAT_PATH: &str = "_path";
const STAT_BASENAME: &str = "_basename";
const STAT_DIR: &str = "_dir";
const STAT_EXT: &str = "_ext";
const STAT_SIZE: &str = "_size";
const STAT_MTIME: &str = "_mtime";
const STAT_CTIME: &str = "_ctime";

/// Compute the filesystem-fact columns for a given file: path-derived
/// (`_path`, `_basename`, `_dir`, `_ext`) and stat-derived (`_size`,
/// `_mtime`, `_ctime`).
fn compute_stat_virtuals(rel_path: &str, abs_path: &Path) -> Row {
    // Read the file's stats once; a missing/unreadable file yields all-`None`,
    // which `stat_virtuals` renders as absent `_size`/`_mtime`/`_ctime`
    // columns. `_mtime`/`_ctime` are `None` when the platform can't supply
    // them (or the value predates the epoch). The pure column-building logic
    // lives in `stat_virtuals`.
    let (size, mtime_secs, ctime_secs) = match std::fs::metadata(abs_path) {
        Ok(metadata) => {
            let to_secs = |t: std::io::Result<std::time::SystemTime>| {
                t.ok()
                    .and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
            };
            (
                Some(metadata.len() as i64),
                to_secs(metadata.modified()),
                to_secs(metadata.created()),
            )
        }
        Err(_) => (None, None, None),
    };
    stat_virtuals(rel_path, size, mtime_secs, ctime_secs)
}

/// Pure core of [`compute_stat_virtuals`]: build the filesystem-fact columns
/// from the relative path plus already-read stat values (each `None` when the
/// corresponding fact is unavailable). Split out so the column-mapping logic
/// is unit-testable without touching the filesystem; the metadata read lives
/// in the caller.
fn stat_virtuals(
    rel_path: &str,
    size: Option<i64>,
    mtime_secs: Option<i64>,
    ctime_secs: Option<i64>,
) -> Row {
    let mut out = Row::new();

    out.insert(STAT_PATH.into(), Value::Text(rel_path.to_string()));

    let pb = Path::new(rel_path);
    if let Some(name) = pb.file_name() {
        out.insert(
            STAT_BASENAME.into(),
            Value::Text(name.to_string_lossy().to_string()),
        );
    }
    if let Some(parent) = pb.parent() {
        out.insert(
            STAT_DIR.into(),
            Value::Text(parent.to_string_lossy().to_string()),
        );
    }
    if let Some(ext) = pb.extension() {
        // Preserve the original case: on case-sensitive filesystems
        // `Photo.JPG` and `photo.jpg` are distinct files. Consumers wanting
        // case-insensitive matching can `LOWER(_ext)` in SQL.
        out.insert(
            STAT_EXT.into(),
            Value::Text(ext.to_string_lossy().into_owned()),
        );
    }

    if let Some(size) = size {
        out.insert(STAT_SIZE.into(), Value::Integer(size));
    }
    if let Some(mtime) = mtime_secs {
        out.insert(STAT_MTIME.into(), Value::Integer(mtime));
    }
    if let Some(ctime) = ctime_secs {
        out.insert(STAT_CTIME.into(), Value::Integer(ctime));
    }

    out
}

/// Merge filesystem-fact columns (stat virtuals + glob captures) into each
/// raw row produced by an extract closure. Auto-injected keys are filtered
/// to those declared in `declared_columns`, so a strict-mode table with a
/// minimal DDL is not broken by virtuals it didn't ask for. User-provided
/// values in `raw_rows` win over auto-injected values: an extract that
/// explicitly emits e.g. `_path` is honored.
fn merge_filesystem_facts(
    raw_rows: Vec<Row>,
    captures: &HashMap<String, String>,
    stat: &Row,
    declared_columns: &[String],
) -> Vec<Row> {
    let declared: std::collections::HashSet<&str> =
        declared_columns.iter().map(String::as_str).collect();

    raw_rows
        .into_iter()
        .map(|raw| {
            let mut merged = Row::new();
            for (k, v) in stat {
                if declared.contains(k.as_str()) {
                    merged.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in captures {
                if declared.contains(k.as_str()) {
                    merged.insert(k.clone(), Value::Text(v.clone()));
                }
            }
            for (k, v) in raw {
                merged.insert(k, v);
            }
            merged
        })
        .collect()
}

// ---------------------------------------------------------------------------
// AsyncDirSQL
// ---------------------------------------------------------------------------

/// Async wrapper around [`DirSQL`] whose constructor returns immediately while
/// the initial scan runs on a background thread.
///
/// Call [`ready()`](AsyncDirSQL::ready) before issuing queries.
#[derive(Clone)]
pub struct AsyncDirSQL {
    inner: Arc<AsyncDirSqlInner>,
}

struct AsyncDirSqlInner {
    db: tokio::sync::OnceCell<std::result::Result<DirSQL, DirSqlError>>,
    ready_notify: tokio::sync::Notify,
}

impl AsyncDirSQL {
    /// Shortcut for `DirSQL::builder().root(root).tables(tables).build_async()`.
    pub fn new(root: impl Into<PathBuf>, tables: Vec<Table>) -> Result<Self> {
        DirSQL::builder().root(root).tables(tables).build_async()
    }

    /// Shortcut for
    /// `DirSQL::builder().root(...).tables(...).ignore(...).build_async()`.
    pub fn with_ignore<I, S>(
        root: impl Into<PathBuf>,
        tables: Vec<Table>,
        ignore: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        DirSQL::builder()
            .root(root)
            .tables(tables)
            .ignore(ignore)
            .build_async()
    }

    /// Shortcut for `DirSQL::builder().config(root/.dirsql.toml).build_async()`.
    pub fn from_config(root: impl Into<PathBuf>) -> Result<Self> {
        DirSQL::builder()
            .config(root.into().join(".dirsql.toml"))
            .build_async()
    }

    /// Shortcut for `DirSQL::builder().config(config_path).build_async()`.
    pub fn from_config_path(config_path: impl AsRef<Path>) -> Result<Self> {
        DirSQL::builder()
            .config(config_path.as_ref().to_path_buf())
            .build_async()
    }

    pub(crate) fn spawn_build(resolved: ResolvedBuild) -> Self {
        let inner = Arc::new(AsyncDirSqlInner {
            db: tokio::sync::OnceCell::new(),
            ready_notify: tokio::sync::Notify::new(),
        });
        let inner_clone = inner.clone();
        thread::spawn(move || {
            let result = DirSQL::build_from_resolved(resolved);
            let _ = inner_clone.db.set(result);
            inner_clone.ready_notify.notify_waiters();
        });
        Self { inner }
    }

    /// Wait until the initial scan has completed. Safe to call multiple times.
    pub async fn ready(&self) -> Result<()> {
        loop {
            if let Some(result) = self.inner.db.get() {
                return match result {
                    Ok(_) => Ok(()),
                    Err(e) => Err(DirSqlError::Lock(format!("init failed: {e}"))),
                };
            }
            self.inner.ready_notify.notified().await;
        }
    }

    pub async fn query(&self, sql: &str) -> Result<Vec<Row>> {
        let db = self.sync()?;
        let sql = sql.to_string();
        tokio::task::spawn_blocking(move || db.query(&sql))
            .await
            .map_err(|e| DirSqlError::Lock(format!("join error: {e}")))?
    }

    pub fn watch(&self) -> Result<WatchStream> {
        self.sync()?.watch()
    }

    /// Forward to the inner [`DirSQL::start_watching`]. Requires init to be
    /// complete.
    pub fn start_watching(&self) -> Result<()> {
        self.sync()?.start_watching()
    }

    /// Forward to the inner [`DirSQL::poll_events`]. Requires init to be
    /// complete.
    pub fn poll_events(&self, timeout: Duration) -> Result<Vec<RowEvent>> {
        self.sync()?.poll_events(timeout)
    }

    /// Access the underlying sync [`DirSQL`]. Errors if init has not completed
    /// (or completed with an error).
    pub fn sync(&self) -> Result<DirSQL> {
        match self.inner.db.get() {
            Some(Ok(db)) => Ok(db.clone()),
            Some(Err(e)) => Err(DirSqlError::Lock(format!("init failed: {e}"))),
            None => Err(DirSqlError::Lock(
                "not ready: call ready().await first".into(),
            )),
        }
    }
}

#[cfg(test)]
mod readonly_tests {
    use super::*;

    #[test]
    fn map_db_error_promotes_write_forbidden() {
        let err = map_db_error(DbError::WriteForbidden);
        // Single-line `matches!` pins the variant without a dead fallback arm.
        assert!(matches!(err, DirSqlError::WriteForbidden), "got: {err:?}");
    }

    #[test]
    fn map_db_error_leaves_sqlite_errors_as_core() {
        let err = map_db_error(DbError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("syntax error".into()),
        )));
        assert!(matches!(err, DirSqlError::Core(_)), "got: {err:?}");
    }

    #[test]
    fn map_db_error_leaves_schema_mismatch_as_core() {
        let err = map_db_error(DbError::SchemaMismatch("nope".into()));
        assert!(matches!(err, DirSqlError::Core(_)), "got: {err:?}");
    }

    #[test]
    fn missing_extension_build_fails_with_extension_error() {
        // The .extension() builder surface loads at startup; a missing file
        // must surface as DirSqlError::Extension (naming the library), not the
        // generic Core(Sqlite) error. (#225 review finding #9; also exercises
        // the .extension() builder method in-process.)
        let dir = tempfile::tempdir().unwrap();
        let err = match DirSQL::builder()
            .root(dir.path())
            .extension(Extension {
                path: "/nonexistent/dirsql-no-such.so".into(),
                entrypoint: None,
            })
            .build()
        {
            Ok(_) => panic!("expected build to fail on a missing extension"),
            Err(e) => e,
        };
        assert!(matches!(err, DirSqlError::Extension { .. }), "got: {err:?}");
        assert!(err.to_string().contains("failed to load extension"));
    }

    #[test]
    fn error_helpers_build_expected_variants() {
        // Exercise the `map_err` helper constructors directly; their runtime
        // call sites only fire on lock poisoning / SQLite failures, which
        // tests can't provoke. Asserting on Display avoids `matches!` (whose
        // dead arm would itself be an uncovered region).
        assert_eq!(
            DirSqlError::lock("x").to_string(),
            "failed to lock shared state: x"
        );
        // `watch`, `config`, `matcher` now wrap a typed StdError to preserve
        // a `source()` chain. Use `std::io::Error` as a portable witness.
        let io = || std::io::Error::new(std::io::ErrorKind::Other, "x");
        let watch_err = DirSqlError::watch(io());
        assert_eq!(watch_err.to_string(), "watcher error: x");
        assert!(StdError::source(&watch_err).is_some());

        let cfg_err = DirSqlError::config(io());
        assert_eq!(cfg_err.to_string(), "config error: x");
        assert!(StdError::source(&cfg_err).is_some());

        let m_err = DirSqlError::matcher(io());
        assert_eq!(m_err.to_string(), "glob matcher error: x");
        assert!(StdError::source(&m_err).is_some());

        // `watch_msg` is the source-less form for internal invariants.
        let watch_msg = DirSqlError::watch_msg("x");
        assert_eq!(watch_msg.to_string(), "watcher error: x");
        assert!(StdError::source(&watch_msg).is_none());

        assert_eq!(
            DirSqlError::sqlite(rusqlite::Error::QueryReturnedNoRows).to_string(),
            "SQLite error: Query returned no rows"
        );
    }
}

#[cfg(test)]
mod internal_tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;
    use tempfile::TempDir;

    /// Deterministic [`FileSystem`] double for unit tests. Backed by a map of
    /// canned [`FileStat`]s (and an optional canned hash); any path not present
    /// stats/hashes as an `io::Error` of kind `NotFound`. Lets the tests of the
    /// persist/reconcile and watch-upsert paths exercise the metadata-read and
    /// racy-window branches without touching the real filesystem (and without
    /// depending on real mtime timing).
    #[derive(Default)]
    struct FakeFs {
        stats: StdHashMap<PathBuf, FileStat>,
        hashes: StdHashMap<PathBuf, [u8; 32]>,
        canonical_roots: StdHashMap<PathBuf, String>,
    }

    impl FakeFs {
        fn with_stat(path: impl Into<PathBuf>, stat: FileStat) -> Self {
            let mut fs = FakeFs::default();
            fs.stats.insert(path.into(), stat);
            fs
        }

        fn set_hash(&mut self, path: impl Into<PathBuf>, hash: [u8; 32]) {
            self.hashes.insert(path.into(), hash);
        }

        /// Builder: register a canned canonicalization for `root`, so the
        /// `watch_root` computation in `finish_build_with_fs` becomes
        /// deterministic without touching the real filesystem or process CWD.
        fn with_canonical_root(
            mut self,
            root: impl Into<PathBuf>,
            canonical: impl Into<String>,
        ) -> Self {
            self.canonical_roots.insert(root.into(), canonical.into());
            self
        }
    }

    impl FileSystem for FakeFs {
        fn stat(&self, path: &Path) -> std::io::Result<FileStat> {
            self.stats.get(path).cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "fake: no such file")
            })
        }

        fn hash(&self, path: &Path) -> std::io::Result<[u8; 32]> {
            self.hashes.get(path).copied().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "fake: no such file")
            })
        }

        fn canonical_root(&self, root: &Path) -> String {
            self.canonical_roots
                .get(root)
                .cloned()
                .unwrap_or_else(|| root.to_string_lossy().into_owned())
        }
    }

    /// A canned [`FileStat`] for unit tests that don't care about specific
    /// values, only that a stat succeeds. `snapshot_ns`-comparable via
    /// `mtime_ns`.
    fn fake_stat() -> FileStat {
        FileStat {
            size: 5,
            mtime_ns: 1_000,
            ctime_ns: 1_000,
            inode: 1,
            dev: 1,
        }
    }

    /// A relative path with a basename, parent dir, and extension exercises the
    /// `Some` arms of `stat_virtuals`' path inspection, plus the `Some` arms of
    /// the size/mtime/ctime inserts. The metadata read that supplies those
    /// values lives in `compute_stat_virtuals` and is covered by the
    /// integration suite (real-file scans).
    #[test]
    fn stat_virtuals_populates_all_fields() {
        let stat = stat_virtuals("nested/sub.txt", Some(5), Some(100), Some(50));
        assert_eq!(stat[STAT_PATH], Value::Text("nested/sub.txt".into()));
        assert_eq!(stat[STAT_BASENAME], Value::Text("sub.txt".into()));
        assert_eq!(stat[STAT_DIR], Value::Text("nested".into()));
        assert_eq!(stat[STAT_EXT], Value::Text("txt".into()));
        assert!(matches!(stat.get(STAT_SIZE), Some(Value::Integer(5))));
        assert!(matches!(stat.get(STAT_MTIME), Some(Value::Integer(100))));
        assert!(matches!(stat.get(STAT_CTIME), Some(Value::Integer(50))));
    }

    /// A bare filename has no parent component and no extension, and a
    /// nonexistent abs path makes `compute_stat_virtuals`' `std::fs::metadata`
    /// read fail (its `Err` arm -> all-`None`). This drives the skip branches:
    /// no `_ext`, no `_size`/`_mtime`/`_ctime`. (Calling the real
    /// `compute_stat_virtuals` with a nonexistent path keeps the test free of a
    /// direct `std::fs` call while still covering the read-failure arm.)
    #[test]
    fn compute_stat_virtuals_skips_absent_fields() {
        let stat = compute_stat_virtuals("bare", Path::new("/nonexistent-xyz/bare"));
        assert_eq!(stat[STAT_PATH], Value::Text("bare".into()));
        assert_eq!(stat[STAT_BASENAME], Value::Text("bare".into()));
        // `Path::new("bare").parent()` is `Some("")`, so `_dir` is an empty
        // string rather than absent; there is no extension and no metadata.
        assert!(!stat.contains_key(STAT_EXT));
        assert!(!stat.contains_key(STAT_SIZE));
        assert!(!stat.contains_key(STAT_MTIME));
        assert!(!stat.contains_key(STAT_CTIME));
    }

    /// An empty relative path has neither a `file_name()` nor a `parent()`,
    /// so the basename and dir `if let Some(..)` blocks both take their
    /// no-match (false) arm: only `_path` is populated.
    #[test]
    fn compute_stat_virtuals_handles_empty_path() {
        let stat = compute_stat_virtuals("", Path::new("/nonexistent-xyz/none"));
        assert_eq!(stat[STAT_PATH], Value::Text(String::new()));
        assert!(
            !stat.contains_key(STAT_BASENAME),
            "empty path has no basename"
        );
        assert!(!stat.contains_key(STAT_DIR), "empty path has no parent dir");
        assert!(!stat.contains_key(STAT_EXT));
    }

    /// `finish_build` defends against a `ScannedFile` whose table has no
    /// registered extract function (a "ghost" entry). With an empty table
    /// list the lookup misses and the defensive `ok_or_else` fires.
    #[test]
    fn finish_build_errors_on_ghost_scanned_file() {
        let dir = TempDir::new().unwrap();
        let matcher = TableMatcher::new(&[], &[]).unwrap();
        let prepared = PreparedBuild {
            root: dir.path().to_path_buf(),
            tables: Vec::new(),
            extensions: Vec::new(),
            matcher,
            scanned_files: vec![ScannedFile {
                rel_path: "ghost.txt".into(),
                table_name: "ghost".into(),
                stat: None,
            }],
            poll_interval: DEFAULT_POLL_INTERVAL,
            persist: None,
        };
        // `.is_err()` keeps the assertion free of an unreachable Ok arm while
        // still executing `finish_build`'s defensive `ok_or_else` path.
        assert!(DirSQL::finish_build(prepared).is_err());
    }

    /// A filesystem event for an ignored path is dropped by
    /// `process_file_event` before it reaches the matcher, returning no row
    /// events. Exercises the `is_ignored` early-return branch directly,
    /// without depending on real filesystem-watch timing. A second,
    /// non-ignored file confirms the same table *does* extract a row when the
    /// path is not ignored -- which also exercises the extract closure body
    /// (so it isn't a dead coverage region).
    #[test]
    fn process_file_event_skips_ignored_paths() {
        let dir = TempDir::new().unwrap();
        let kept = dir.path().join("keep.txt");
        // Inject a fake fs so the non-ignored path's stat read succeeds without
        // staging a real file. The ignored path is dropped before any stat.
        let fake = FakeFs::with_stat(kept.clone(), fake_stat());
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::new(
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text("x".into()),
                    )])]
                },
            )],
            vec!["skip/**"],
            Arc::new(fake),
        )
        .unwrap();

        // Ignored path: dropped before the matcher, no events.
        let ignored = dir.path().join("skip").join("a.txt");
        let events = db.process_file_event(FileEvent::Created(ignored));
        assert!(events.is_empty(), "ignored path must produce no events");

        // Non-ignored path: the extract closure runs and yields one insert.
        let events = db.process_file_event(FileEvent::Created(kept));
        assert_eq!(events.len(), 1, "non-ignored path must produce one event");
    }

    // -----------------------------------------------------------------------
    // #250: canonical `watch_root` and the strip-prefix fallbacks.
    //
    // The canonicalization runs through the `FileSystem` seam, so these tests
    // inject a `FakeFs` with a canned `canonical_root` mapping instead of
    // mutating the process CWD or staging real files (#233).
    // -----------------------------------------------------------------------

    /// Building with a **relative** root canonicalizes `watch_root` to an
    /// absolute path while leaving `root` (and therefore `config()` / `_path`)
    /// exactly as the caller supplied it. This is the core of the #250 fix:
    /// `start_watching` watches `watch_root`, so `notify` never sees `.`.
    #[test]
    fn relative_root_canonicalizes_watch_root_only() {
        // FakeFs canonicalizes the relative root `.` to a fixed absolute
        // string, so the watch_root computation is deterministic with no CWD
        // juggling.
        let fake = FakeFs::default().with_canonical_root(".", "/ws/canonical");
        let db = DirSQL::with_ignore_and_fs(
            ".",
            vec![Table::new("CREATE TABLE t (x TEXT)", "*.txt", |_| vec![])],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        // `root` is preserved verbatim.
        assert_eq!(db.inner.root, PathBuf::from("."));
        // `watch_root` is absolute and points at the canonical dir.
        assert!(
            db.inner.watch_root.is_absolute(),
            "watch_root must be absolute, got {:?}",
            db.inner.watch_root
        );
        assert_eq!(db.inner.watch_root, PathBuf::from("/ws/canonical"));
    }

    /// With an absolute root the canonical `watch_root` equals the (already
    /// canonical) root on this platform, and `process_file_event` strips that
    /// prefix to yield a root-relative `_path` — the first `strip_prefix`
    /// (watch_root) arm.
    #[test]
    fn process_file_event_strips_watch_root_prefix() {
        let root = PathBuf::from("/ws");
        let abs = root.join("nested").join("a.txt");
        // FakeFs canonicalizes the root to itself (already-canonical case) and
        // stats the event path so the upsert's existence check passes — no real
        // file is staged.
        let fake = FakeFs::with_stat(abs.clone(), fake_stat()).with_canonical_root(&root, "/ws");
        let db = DirSQL::with_ignore_and_fs(
            &root,
            vec![Table::new(
                "CREATE TABLE items (name TEXT, _path TEXT)",
                "**/*.txt",
                |_| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text("x".into()),
                    )])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        let events = db.process_file_event(FileEvent::Created(abs));
        assert_eq!(events.len(), 1, "expected one insert: {events:?}");
        match &events[0] {
            RowEvent::Insert { row, .. } => {
                assert_eq!(
                    row.get("_path"),
                    Some(&Value::Text("nested/a.txt".to_string())),
                    "watch_root prefix must be stripped to a root-relative path"
                );
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    /// When an event path lies under the user-supplied `root` but not under
    /// the canonical `watch_root`, the `.or_else` fallback strips `root`
    /// instead. We force that split by pointing `watch_root` at a sibling that
    /// is not a prefix of the event path, leaving `root` as the real dir.
    #[test]
    fn process_file_event_falls_back_to_root_prefix() {
        let root = PathBuf::from("/ws");
        let abs = root.join("b.txt");
        let fake = FakeFs::with_stat(abs.clone(), fake_stat()).with_canonical_root(&root, "/ws");
        let mut db = DirSQL::with_ignore_and_fs(
            &root,
            vec![Table::new(
                "CREATE TABLE items (name TEXT, _path TEXT)",
                "**/*.txt",
                |_| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text("x".into()),
                    )])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        // Repoint watch_root to a non-prefix sibling so the first strip misses
        // and the `.or_else(root)` arm runs. `root` stays the real dir.
        Arc::get_mut(&mut db.inner).unwrap().watch_root = root.join("does-not-prefix");

        let events = db.process_file_event(FileEvent::Created(abs));
        assert_eq!(events.len(), 1, "expected one insert: {events:?}");
        match &events[0] {
            RowEvent::Insert { row, .. } => {
                assert_eq!(
                    row.get("_path"),
                    Some(&Value::Text("b.txt".to_string())),
                    "root fallback must strip the user-supplied root prefix"
                );
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    /// When the event path is under neither `watch_root` nor `root`, the final
    /// `unwrap_or(&abs_path)` arm keeps the absolute path. A path that matches
    /// no table glob then yields no events, but the strip fallback is still
    /// executed — we assert the no-event outcome to pin the arm without relying
    /// on a row.
    #[test]
    fn process_file_event_keeps_absolute_path_when_no_prefix_matches() {
        let root = PathBuf::from("/ws");
        let fake = FakeFs::default().with_canonical_root(&root, "/ws");
        let db = DirSQL::with_ignore_and_fs(
            &root,
            vec![Table::new(
                "CREATE TABLE items (name TEXT)",
                "*.txt",
                |_| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text("x".into()),
                    )])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        // A path outside both roots: neither strip matches, so the absolute
        // path is used as the relative path. It does not match `*.txt` at the
        // root, so no events are produced. (The non-matching glob returns before
        // any stat, so the FakeFs needs no entry for this path.)
        let outside = PathBuf::from("/some/elsewhere/c.md");
        let events = db.process_file_event(FileEvent::Created(outside));
        assert!(
            events.is_empty(),
            "unmatched absolute path must produce no events: {events:?}"
        );
    }

    /// Drive `reconcile_scan` directly with a cached file whose
    /// `snapshot_ns <= mtime_ns`, forcing the racy-window hash-confirm branch.
    /// With a matching content hash the file is trusted.
    #[test]
    fn reconcile_scan_hash_confirms_in_racy_window() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let stat = fake_stat();
        let live_hash = [7u8; 32];
        // Fake fs: canned stat + matching hash, so the racy-window hash-confirm
        // branch sees a live hash equal to the cached one and trusts the file.
        let mut fake = FakeFs::with_stat(abs.clone(), stat.clone());
        fake.set_hash(abs.clone(), live_hash);

        let mut cached = HashMap::new();
        cached.insert(
            "a.txt".to_string(),
            CachedFile {
                rel_path: "a.txt".into(),
                table_name: "t".into(),
                stat: stat.clone(),
                content_hash: Some(live_hash),
                // snapshot_ns <= mtime_ns => inside the racy window.
                snapshot_ns: stat.mtime_ns,
            },
        );
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached,
            expected_meta: HashMap::new(),
            cold_rebuild: false,
        };
        let scanned = vec![(abs.clone(), "t".to_string())];
        let (to_parse, trusted, deleted) =
            reconcile_scan(dir.path(), scanned, &ctx, &fake).unwrap();
        assert!(to_parse.is_empty());
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].rel_path, "a.txt");
        assert!(deleted.is_empty());
    }

    /// Same racy-window entry but with no stored content hash falls through
    /// the `_ => false` arm, so the file is NOT trusted and is re-parsed.
    #[test]
    fn reconcile_scan_racy_window_without_hash_reparses() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("b.txt");
        let stat = fake_stat();
        // The live hash is available (file present) but the cache stored no
        // content hash, so the `(Some(live), None)` pair falls through the
        // `_ => false` arm: the file is NOT trusted and is re-parsed.
        let mut fake = FakeFs::with_stat(abs.clone(), stat.clone());
        fake.set_hash(abs.clone(), [9u8; 32]);

        let mut cached = HashMap::new();
        cached.insert(
            "b.txt".to_string(),
            CachedFile {
                rel_path: "b.txt".into(),
                table_name: "t".into(),
                stat: stat.clone(),
                content_hash: None,
                snapshot_ns: stat.mtime_ns,
            },
        );
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached,
            expected_meta: HashMap::new(),
            cold_rebuild: false,
        };
        let scanned = vec![(abs.clone(), "t".to_string())];
        let (to_parse, trusted, _deleted) =
            reconcile_scan(dir.path(), scanned, &ctx, &fake).unwrap();
        assert_eq!(to_parse.len(), 1);
        assert!(trusted.is_empty());
    }

    /// `reconcile_scan` stats every scanned path; a path that has vanished
    /// makes `std::fs::metadata` fail and the `?` propagates the error.
    #[test]
    fn reconcile_scan_errors_when_file_vanished() {
        let dir = TempDir::new().unwrap();
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached: HashMap::new(),
            expected_meta: HashMap::new(),
            cold_rebuild: false,
        };
        let missing = dir.path().join("ghost.txt");
        let scanned = vec![(missing, "t".to_string())];
        // An empty fake fs stats every path as NotFound; the `?` in
        // `reconcile_scan` propagates that error.
        let fake = FakeFs::default();
        assert!(reconcile_scan(dir.path(), scanned, &ctx, &fake).is_err());
    }

    /// Exercise the production [`RealFs`] [`FileSystem`] impl directly so its
    /// `stat`/`hash` method bodies are covered without the integration suite
    /// having to deterministically land in `reconcile_scan`'s racy window.
    /// Both methods run against a path that does not exist (inside a temp dir,
    /// so no direct `std::fs` call lives in the test): each delegates to its
    /// real backing (`std::fs::metadata` / `hash_file`) and surfaces the
    /// resulting `NotFound` error, executing the body either way.
    #[test]
    fn real_fs_delegates_stat_and_hash() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.txt");
        let fs = RealFs;
        assert!(
            fs.stat(&missing).is_err(),
            "stat of a missing path must error"
        );
        assert!(
            fs.hash(&missing).is_err(),
            "hash of a missing path must error"
        );
        // `canonical_root` of a nonexistent path can't canonicalize, so it
        // takes the literal fallback (the path's lossy string) — exercising the
        // RealFs delegation to `persist::canonical_root` without a direct
        // `std::fs` call in the test.
        assert_eq!(
            fs.canonical_root(&missing),
            missing.to_string_lossy(),
            "canonical_root must fall back to the literal path when it can't canonicalize"
        );
    }

    // -----------------------------------------------------------------------
    // Lock-poison error arms
    //
    // The `.lock().map_err(DirSqlError::lock)?` (and `match self.inner.*.lock()`)
    // arms only fire when a mutex is poisoned -- i.e. a thread panicked while
    // holding the guard. We provoke that deterministically with a scoped
    // thread that locks the mutex and panics, then assert the error/Display
    // surface. These tests reach into the private `inner` mutexes, which is
    // why they live in the in-crate `internal_tests` module rather than the
    // public-API integration suite.
    // -----------------------------------------------------------------------

    /// Poison a mutex by panicking while holding its guard. `catch_unwind` on
    /// the current thread does this without `std::thread` (the `unit lint`
    /// isolation rule keeps effectful std out of unit tests): the guard's
    /// `Drop` runs during unwinding and marks the mutex poisoned.
    fn poison<T: Send>(m: &Mutex<T>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("poison");
        }));
        assert!(m.is_poisoned(), "mutex should be poisoned");
    }

    /// Build a tableless `DirSQL` over an empty temp dir. These tests only
    /// need a live instance whose inner mutexes can be poisoned, so there is no
    /// table or file to stage (which keeps `std::fs` out of this unit module).
    /// Extract-closure coverage lives in the `process_file_event_*` tests.
    fn simple_db() -> (TempDir, DirSQL) {
        let dir = TempDir::new().unwrap();
        let db =
            DirSQL::with_ignore(dir.path(), Vec::<Table>::new(), Vec::<String>::new()).unwrap();
        (dir, db)
    }

    /// Build a one-table `DirSQL` over a temp dir with a single matching file
    /// already written. Returns the db plus the file's absolute and relative
    /// paths so callers can drive `handle_*` directly.
    fn upsert_fixture() -> (TempDir, DirSQL, PathBuf, String) {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        // Inject a fake fs that stats the fixture path successfully (so
        // `handle_upsert`'s vanished-file guard passes) without staging a real
        // file. Any other path stats as NotFound, which the vanished-file test
        // relies on.
        let fake = FakeFs::with_stat(abs.clone(), fake_stat());
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::new(
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text("x".into()),
                    )])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();
        (dir, db, abs, "a.txt".to_string())
    }

    #[test]
    fn query_surfaces_lock_poison() {
        let (_dir, db) = simple_db();
        poison(&db.inner.db);
        let err = db.query("SELECT 1").unwrap_err();
        // Exercises both `query`'s `?` arm and the `DirSqlError::lock` helper.
        assert!(err.to_string().starts_with("failed to lock"), "got: {err}");
    }

    #[test]
    fn start_watching_surfaces_lock_poison() {
        let (_dir, db) = simple_db();
        poison(&db.inner.watcher);
        let err = db.start_watching().unwrap_err();
        assert!(err.to_string().starts_with("failed to lock"), "got: {err}");
    }

    #[test]
    fn poll_once_surfaces_lock_poison() {
        let (_dir, db) = simple_db();
        // Start the watcher first so the guard is `Some`, then poison the
        // mutex and call the private `poll_once` directly (the public
        // `poll_events` would trip `start_watching`'s own lock first).
        db.start_watching().unwrap();
        poison(&db.inner.watcher);
        let err = db.poll_once(Duration::from_millis(0)).unwrap_err();
        assert!(err.to_string().starts_with("failed to lock"), "got: {err}");
    }

    /// A `RowEvent::Error` whose message reports a lock failure. Asserts on the
    /// Debug rendering so there is no `match`/`else` fallback arm. The
    /// `handle_*` arms forward the raw `PoisonError` (`e.to_string()`), whose
    /// std Display contains "poisoned lock".
    fn assert_single_lock_error(events: &[RowEvent]) {
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("poisoned lock"), "expected poison text: {dbg}");
    }

    #[test]
    fn handle_delete_surfaces_file_rows_poison() {
        let (_dir, db, _abs, rel) = upsert_fixture();
        poison(&db.inner.file_rows);
        let events = db.handle_delete("items", &rel);
        assert_single_lock_error(&events);
    }

    #[test]
    fn handle_delete_surfaces_db_poison() {
        let (_dir, db, _abs, rel) = upsert_fixture();
        // Only the db mutex is poisoned; the file_rows lock succeeds first,
        // so the error comes from the db-lock arm.
        poison(&db.inner.db);
        let events = db.handle_delete("items", &rel);
        assert_single_lock_error(&events);
    }

    #[test]
    fn handle_delete_surfaces_db_failure() {
        let (_dir, db, _abs, _rel) = upsert_fixture();
        // No SQL table named `ghost` exists, so `delete_rows_by_file` issues a
        // `DELETE FROM ghost ...` that fails with "no such table". That drives
        // the `delete_result` Err arm of `handle_delete`.
        let events = db.handle_delete("ghost", "whatever.txt");
        assert_eq!(events.len(), 1, "expected one error event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("no such table"), "expected a SQL error: {dbg}");
    }

    #[test]
    fn handle_upsert_surfaces_db_poison() {
        let (_dir, db, abs, rel) = upsert_fixture();
        poison(&db.inner.db);
        let events = db.handle_upsert("items", &abs, &rel);
        assert_single_lock_error(&events);
    }

    #[test]
    fn handle_upsert_surfaces_file_rows_poison() {
        let (_dir, db, abs, rel) = upsert_fixture();
        // db is healthy, so metadata/extract/get_table_columns/normalize all
        // succeed and the error originates at the file_rows-lock arm.
        poison(&db.inner.file_rows);
        let events = db.handle_upsert("items", &abs, &rel);
        assert_single_lock_error(&events);
    }

    // ----- handle_upsert clean early-returns --------------------------------

    #[test]
    fn handle_upsert_returns_empty_when_file_vanished() {
        let (dir, db, _abs, _rel) = upsert_fixture();
        let missing = dir.path().join("gone.txt");
        // The file never existed, so `std::fs::metadata` returns NotFound and
        // `handle_upsert` returns no events.
        let events = db.handle_upsert("items", &missing, "gone.txt");
        assert!(events.is_empty(), "vanished file must produce no events");
    }

    #[test]
    fn handle_upsert_returns_empty_for_unknown_table() {
        let (_dir, db, abs, rel) = upsert_fixture();
        // The file exists, but no extract closure is registered for this table
        // name, so the extract-map lookup misses and we return no events.
        let events = db.handle_upsert("not_a_table", &abs, &rel);
        assert!(events.is_empty(), "unknown table must produce no events");
    }

    #[test]
    fn handle_upsert_surfaces_normalize_error_in_strict_mode() {
        // A strict table rejects rows with columns its DDL doesn't declare.
        // The extract emits an undeclared `extra` column, so `normalize_row`
        // returns a SchemaMismatch and `handle_upsert` reports it as a single
        // RowEvent::Error (the strict-mode normalize-error arm).
        //
        // The dir is empty at build time so the initial scan matches nothing;
        // the file is created afterwards and reaches the DB only through
        // `handle_upsert`, isolating the arm under test.
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        // Inject a fake fs so the strict table's `handle_upsert` stat read
        // succeeds without staging a real file; the normalize-error arm is the
        // arm under test.
        let fake = FakeFs::with_stat(abs.clone(), fake_stat());
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::strict(
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| {
                    vec![Row::from_iter([
                        ("name".to_string(), Value::Text("ok".into())),
                        ("extra".to_string(), Value::Text("nope".into())),
                    ])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        let events = db.handle_upsert("items", &abs, "a.txt");
        assert_eq!(events.len(), 1, "expected one error event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(
            dbg.contains("extra columns"),
            "expected a strict schema-mismatch message: {dbg}"
        );
    }

    // ----- run_channel_loop error arm --------------------------------------

    #[test]
    fn run_channel_loop_emits_error_event_on_poll_failure() {
        let (_dir, db) = simple_db();
        // Start the watcher, then poison it so the loop's first `poll_once`
        // returns Err, driving the `Err(e)` arm: it pushes one RowEvent::Error
        // and returns.
        db.start_watching().unwrap();
        poison(&db.inner.watcher);

        let (tx, mut rx) = unbounded();
        run_channel_loop(db, tx);

        let event = rx.try_recv().expect("expected an error event");
        let dbg = format!("{event:?}");
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("failed to lock"), "expected lock text: {dbg}");
        // The loop returns after the error, so the channel is now drained and
        // its sender dropped; a further `try_recv` reports the empty channel.
        assert!(rx.try_recv().is_err(), "loop should have ended");
    }
}

#[cfg(test)]
mod command_rows_tests {
    use super::*;

    #[test]
    fn parses_an_array_of_row_objects() {
        let rows = parse_command_rows(r#"[{"id":"a","n":1},{"id":"b","n":2}]"#).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], Value::Text("a".into()));
        assert_eq!(rows[0]["n"], Value::Integer(1));
        assert_eq!(rows[1]["id"], Value::Text("b".into()));
        assert_eq!(rows[1]["n"], Value::Integer(2));
    }

    #[test]
    fn parses_an_empty_array_to_no_rows() {
        assert_eq!(parse_command_rows("[]").unwrap(), Vec::<Row>::new());
    }

    #[test]
    fn maps_every_json_value_type_including_nested_to_text_json() {
        let rows = parse_command_rows(
            r#"[{"nul":null,"t":true,"f":false,"i":42,"r":1.5,"s":"hi","arr":[1,2],"obj":{"k":"v"}}]"#,
        )
        .unwrap();
        let row = &rows[0];
        assert_eq!(row["nul"], Value::Null);
        assert_eq!(row["t"], Value::Integer(1));
        assert_eq!(row["f"], Value::Integer(0));
        assert_eq!(row["i"], Value::Integer(42));
        assert_eq!(row["r"], Value::Real(1.5));
        assert_eq!(row["s"], Value::Text("hi".into()));
        assert_eq!(row["arr"], Value::Text("[1,2]".into()));
        assert_eq!(row["obj"], Value::Text(r#"{"k":"v"}"#.into()));
    }

    #[test]
    fn a_number_that_does_not_fit_i64_becomes_real() {
        // 10^19 exceeds i64::MAX (~9.2e18) but fits u64, so `as_i64` is None and
        // it falls through to `Real`.
        let rows = parse_command_rows(r#"[{"big":10000000000000000000}]"#).unwrap();
        assert!(matches!(rows[0]["big"], Value::Real(_)));
    }

    #[test]
    fn a_non_array_payload_is_an_error() {
        let err = parse_command_rows(r#"{"id":"a"}"#).unwrap_err();
        assert!(err.contains("array"), "got: {err}");
    }

    #[test]
    fn an_element_that_is_not_an_object_is_an_error() {
        let err = parse_command_rows(r#"[{"id":"a"}, 3]"#).unwrap_err();
        assert!(err.contains("object"), "got: {err}");
    }

    #[test]
    fn invalid_json_is_an_error() {
        let err = parse_command_rows("not json at all").unwrap_err();
        assert!(err.contains("invalid JSON"), "got: {err}");
    }

    #[test]
    fn json_to_value_maps_each_variant() {
        assert_eq!(json_to_value(&serde_json::Value::Null), Value::Null);
        assert_eq!(json_to_value(&serde_json::json!(true)), Value::Integer(1));
        assert_eq!(json_to_value(&serde_json::json!(false)), Value::Integer(0));
        assert_eq!(json_to_value(&serde_json::json!(7)), Value::Integer(7));
        assert_eq!(json_to_value(&serde_json::json!(2.5)), Value::Real(2.5));
        assert_eq!(
            json_to_value(&serde_json::json!("x")),
            Value::Text("x".into())
        );
        assert_eq!(
            json_to_value(&serde_json::json!([1, 2])),
            Value::Text("[1,2]".into())
        );
    }
}
