//! `dirsql` — an ephemeral SQL index over a local directory.
//!
//! The published crate surface is intentionally small: [`DirSQL`], [`AsyncDirSQL`],
//! [`Table`], [`Row`], [`RowEvent`], [`Value`], [`DirSqlError`]. Internal modules
//! (`config`, `db`, `differ`, `matcher`, `parser`, `scanner`, `watcher`) are
//! marked `#[doc(hidden)]`: they remain callable so in-crate benches and language
//! bindings in this workspace can reach them, but they are not part of the
//! stable public API.

/// Reusable command runner backing the command-backed events.
pub mod command;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod db;
#[doc(hidden)]
pub mod differ;
#[doc(hidden)]
pub mod infer;
#[doc(hidden)]
pub mod matcher;
#[doc(hidden)]
pub mod parsed_vtab;
#[doc(hidden)]
pub mod path_table;
#[doc(hidden)]
pub mod persist;
#[doc(hidden)]
pub mod scanner;
#[doc(hidden)]
pub mod vtab;
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
    hash_file, meta_is_compatible, now_ns, read_cached_files, read_meta, resolve_persist_path,
    upsert_file, write_meta,
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

/// The escalation scaffold `dirsql init` writes verbatim: one named
/// `[[table]]` (glob + DDL + a real `on-file` hook) demonstrating how to pull
/// structured rows out of files, rather than duplicating the zero-config
/// path-table floor (`SELECT * FROM './'`). The `--include-default` launcher
/// path also seeds this table's glob/DDL. Carrying a genuine hook keeps it a
/// valid config even once hook-less `[[table]]` entries become a load error.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("default_config.toml");

type BoxError = Box<dyn StdError + Send + Sync + 'static>;
type OnFileFn = dyn Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static;

/// One file whose on-file hook failed, carried so a scan can report every
/// failure rather than only whichever came first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnFileFailure {
    /// Path relative to the scan root.
    pub path: String,
    /// The hook's error, as rendered by its `Display`.
    pub message: String,
}

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

    #[error("on-file error for {path}: {message}")]
    OnFile { path: String, message: String },

    /// Several files' on-file hooks failed during one scan. A scan attempts
    /// every matched file, so more than one can fail; reporting only the first
    /// would make the rest invisible until it was fixed and the scan re-run.
    #[error(
        "{} files failed their on-file hook:\n{}",
        .failures.len(),
        .failures
            .iter()
            .map(|OnFileFailure { path, message }| format!("  {path}: {message}"))
            .collect::<Vec<_>>()
            .join("\n")
    )]
    OnFileMany { failures: Vec<OnFileFailure> },

    #[error("config error: {message}")]
    Config {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    #[error(
        "glob capture `{{{placeholder}}}` collides with declared column `{column}`: \
         captures no longer populate columns, so `{column}` would always be NULL. \
         Remove `{column}` from the table's DDL, or emit its value from the on-file \
         hook by splitting `{{path}}` yourself."
    )]
    CaptureColumnCollision { placeholder: String, column: String },

    #[error(
        "query() only accepts read-only statements; SQLite classified this statement as a write"
    )]
    WriteForbidden,
}

impl DirSqlError {
    fn lock(e: impl std::fmt::Display) -> Self {
        DirSqlError::Lock(e.to_string())
    }

    fn watch<E: StdError + Send + Sync + 'static>(e: E) -> Self {
        DirSqlError::Watch {
            message: e.to_string(),
            source: Some(Box::new(e)),
        }
    }

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

/// A single table definition: DDL + glob + on_file callback.
///
/// The `on_file` callback receives the **absolute filesystem path** of each
/// matched file and returns the rows that file contributes. dirsql does not
/// read file contents itself; a callback that needs the file body reads it
/// inside the closure (`std::fs::read_to_string(path)` etc.). Callbacks that
/// derive columns purely from the path or from filesystem facts never touch
/// the file at all.
///
/// Use [`Table::new`] for infallible callbacks or [`Table::try_new`] when the
/// callback can itself fail (bad file content, IO errors inside the callback,
/// etc.). [`Table::strict`] rejects rows that don't match the DDL columns
/// exactly.
#[derive(Clone)]
pub struct Table {
    pub ddl: String,
    pub glob: String,
    pub strict: bool,
    on_file: Arc<OnFileFn>,
}

impl Table {
    pub fn new<F>(ddl: impl Into<String>, glob: impl Into<String>, on_file: F) -> Self
    where
        F: Fn(&str) -> Vec<Row> + Send + Sync + 'static,
    {
        Self::try_new(ddl, glob, move |path| {
            Ok::<Vec<Row>, BoxError>(on_file(path))
        })
    }

    pub fn strict<F>(ddl: impl Into<String>, glob: impl Into<String>, on_file: F) -> Self
    where
        F: Fn(&str) -> Vec<Row> + Send + Sync + 'static,
    {
        let mut table = Self::new(ddl, glob, on_file);
        table.strict = true;
        table
    }

    pub fn try_new<F>(ddl: impl Into<String>, glob: impl Into<String>, on_file: F) -> Self
    where
        F: Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static,
    {
        Self {
            ddl: ddl.into(),
            glob: glob.into(),
            on_file: Arc::new(on_file),
            strict: false,
        }
    }
}

struct DirSqlInner {
    db: Mutex<Db>,
    root: PathBuf,
    /// Canonicalized `root`, used **only** for the live filesystem watcher:
    /// `notify` misbehaves on relative paths (it may deliver no events, or
    /// deliver them under the cwd-joined path so the relative prefix no
    /// longer strips). Literal fallback when canonicalization fails (e.g. a
    /// not-yet-created root); the user's `root` — and therefore the initial
    /// scan and the `path` column — stays byte-for-byte unchanged.
    watch_root: PathBuf,
    matcher: TableMatcher,
    on_file_map: HashMap<String, Arc<OnFileFn>>,
    strict_map: HashMap<String, bool>,
    watcher: Mutex<Option<Watcher>>,
    /// Locks out [`DirSQL::watch`] once [`DirSQL::poll_events`] has run:
    /// both would drain the same underlying watcher.
    poll_used: AtomicBool,
    /// Locks out [`DirSQL::poll_events`] once [`DirSQL::watch`] has spawned
    /// its background thread.
    watch_thread_started: AtomicBool,
    poll_interval: Duration,
    /// Filesystem seam: [`RealFs`] in production; unit tests inject a double.
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
    /// `.dirsql.toml`, pass the config path via `.config(path)`; to set the
    /// index root, use `.root(path)`; to add tables programmatically, use
    /// `.table(t)` / `.tables(ts)`. The index root is the explicit `.root(...)`
    /// when given, else the process cwd — the config file's location never
    /// contributes.
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

    /// Shortcut for `DirSQL::builder().config(config_path).build()`.
    ///
    /// With no explicit `.root()`, the index roots at the process cwd, not the
    /// config file's parent directory. To read `<root>/.dirsql.toml`, pass it
    /// explicitly: `DirSQL::from_config_path(root.join(".dirsql.toml"))` (the
    /// implicit root-joining `from_config(root)` shortcut was removed in #603).
    pub fn from_config_path(config_path: impl AsRef<Path>) -> Result<Self> {
        DirSQL::builder()
            .config(config_path.as_ref().to_path_buf())
            .build()
    }

    /// Run a SQL query against the ephemeral database.
    ///
    /// Only read-only statements are accepted. Each statement is prepared on
    /// SQLite and then classified via `sqlite3_stmt_readonly`; anything that
    /// SQLite itself flags as a write — `INSERT`, `UPDATE`, `DELETE`, `DROP`,
    /// `CREATE`, `ALTER`, `REPLACE`, `VACUUM`, `ANALYZE`, etc. — is rejected
    /// with [`DirSqlError::WriteForbidden`] before any rows are produced. This
    /// keeps the ephemeral index consistent with the on-disk files that back
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
            // user-supplied one — `notify` misbehaves on relative paths.
            let watcher = Watcher::new(&self.inner.watch_root).map_err(DirSqlError::watch)?;
            *guard = Some(watcher);
        }
        Ok(())
    }

    /// Poll-based watch API. Blocks up to `timeout` waiting for the next
    /// filesystem event, then drains any additional events that arrived during
    /// processing, applying all of them to the ephemeral database. Returns the
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
    /// invoke the `on_file` callback off the host thread (e.g. the napi-rs
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

    /// Apply a batch of raw file events through the on_file/DB update
    /// pipeline. Counterpart to [`wait_file_events`](Self::wait_file_events).
    /// Runs the `on_file` callback inline, so the caller must invoke this on
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
        // Events arrive under the canonical `watch_root`, so strip that
        // first; fall back to the user-supplied `root` (the already-canonical
        // /absolute-root case), then to the raw absolute path.
        let rel_path_buf = abs_path
            .strip_prefix(&self.inner.watch_root)
            .or_else(|_| abs_path.strip_prefix(&self.inner.root))
            .unwrap_or(&abs_path)
            .to_path_buf();

        if self.inner.matcher.is_ignored(&rel_path_buf) {
            return Vec::new();
        }

        // Fan-out: dispatch the event to every table whose glob matches, and
        // concatenate the resulting row events. Cross-table event order is
        // unspecified. An `on_file` failure produces an error event for that
        // table only; the other matching tables still process the event.
        let matches = self.inner.matcher.match_all(&rel_path_buf);
        if matches.is_empty() {
            return Vec::new();
        }
        let rel_path = rel_path_buf.to_string_lossy().to_string();

        let mut events = Vec::new();
        for m in matches {
            match &event {
                FileEvent::Deleted(_) => {
                    events.extend(self.handle_delete(&m.table_name, &rel_path));
                }
                FileEvent::Created(_) | FileEvent::Modified(_) => {
                    events.extend(self.handle_upsert(&m.table_name, &abs_path, &rel_path));
                }
            }
        }
        events
    }

    fn handle_delete(&self, table: &str, rel_path: &str) -> Vec<RowEvent> {
        // Snapshot the file's rows before deleting them — the Delete events
        // carry the old-row payloads. Read and delete under one lock
        // acquisition so a concurrent event for the same file can't
        // interleave between them.
        let old_rows = {
            let db = match self.inner.db.lock() {
                Ok(db) => db,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
            let old_rows = match db.get_rows_by_file(table, rel_path) {
                Ok(rows) => rows,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
            // Wrap the delete in a transaction so it either succeeds completely
            // or rolls back; no partial deletes.
            let _tx = match db.conn().unchecked_transaction() {
                Ok(tx) => tx,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
            if let Err(e) = db.delete_rows_by_file(table, rel_path) {
                return vec![error_event(Some(table), rel_path, e.to_string())];
            }
            if let Err(e) = _tx.commit() {
                return vec![error_event(Some(table), rel_path, e.to_string())];
            }
            old_rows
        };

        differ::diff(table, Some(&old_rows), None, rel_path)
    }

    fn handle_upsert(&self, table: &str, abs_path: &Path, rel_path: &str) -> Vec<RowEvent> {
        // The path may have vanished between the watcher event and now, or be a
        // directory (a `mkdir` under the root matches a `**/*` glob). Only
        // regular files become rows — mirror the initial scan, which skips
        // non-files.
        match self.inner.fs.is_file(abs_path) {
            Ok(true) => {}
            Ok(false) => return Vec::new(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        }

        let on_file = match self.inner.on_file_map.get(table) {
            Some(e) => e,
            None => return Vec::new(),
        };

        let raw_rows = match on_file(&abs_path.to_string_lossy()) {
            Ok(r) => r,
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        let strict = *self.inner.strict_map.get(table).unwrap_or(&false);

        // Normalize, snapshot the file's previous rows, and apply the
        // delete+insert under one lock acquisition, so a concurrent event for
        // the same file can't interleave between the read and the write.
        let (old_rows, new_rows) = {
            let db = match self.inner.db.lock() {
                Ok(g) => g,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
            let mut new_rows = Vec::with_capacity(raw_rows.len());
            for raw in &raw_rows {
                match db.normalize_row(table, raw, strict) {
                    Ok(row) => new_rows.push(row),
                    Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
                }
            }

            let old_rows = match db.get_rows_by_file(table, rel_path) {
                Ok(rows) => rows,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };

            // Wrap the delete+insert in a transaction so they commit together;
            // a failed multi-row update rolls back completely instead of leaving
            // partial rows.
            let _tx = match db.conn().unchecked_transaction() {
                Ok(tx) => tx,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };

            let db_result = db.delete_rows_by_file(table, rel_path).and_then(|_| {
                for (i, row) in new_rows.iter().enumerate() {
                    db.insert_row(table, row, rel_path, i)?;
                }
                Ok(())
            });
            if let Err(e) = db_result {
                return vec![error_event(Some(table), rel_path, e.to_string())];
            }

            if let Err(e) = _tx.commit() {
                return vec![error_event(Some(table), rel_path, e.to_string())];
            }

            (old_rows, new_rows)
        };

        differ::diff(table, Some(&old_rows), Some(&new_rows), rel_path)
    }

    pub(crate) fn build_from_resolved(resolved: ResolvedBuild) -> Result<Self> {
        let prepared = Self::prepare_resolved(resolved)?;
        Self::finish_build(prepared)
    }

    /// Test-seam build path: identical to [`build_from_resolved`] but stores
    /// the supplied [`FileSystem`] double on the resulting instance. The
    /// prepare phase still uses [`RealFs`] (it has no instance yet), so
    /// callers must build over an empty temp dir.
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
            hint_legacy_files_table: false,
            path_table_parser: None,
        };
        let prepared = Self::prepare_resolved(resolved)?;
        Self::finish_build_with_fs(prepared, fs)
    }

    /// Split-phase construction — part 1. Performs all I/O that is safe to run
    /// off the host's main thread: validates DDL, compiles the matcher, walks
    /// the directory, opens the persistent cache (when enabled) and decides
    /// which files need re-parsing. Does **not** read file contents and does
    /// **not** invoke `on_file`.
    ///
    /// Pair with [`finish_build`](Self::finish_build) to complete construction
    /// on a thread where the `on_file` callback can safely execute (e.g. the
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
            hint_legacy_files_table,
            path_table_parser,
        } = resolved;

        let (matcher, table_names) = compile_matcher(&tables, &ignore)?;

        // Resolve the persistent context before scanning, so the scan can
        // consult the cached file index.
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

        let scanned = scan_directory(&root, &matcher);

        // When persist is enabled, files whose stat tuple matches the cache
        // (and that pass the racy-window check) are trusted instead of
        // re-parsed.
        let (scanned_files, _trusted, deleted) = match &persist_ctx {
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
            ignore,
            scanned_files,
            hint_legacy_files_table,
            persist: persist_ctx.map(|ctx| PreparedPersist {
                db: ctx.db,
                deleted,
                meta: ctx.expected_meta,
            }),
            poll_interval,
            path_table_parser,
        })
    }

    /// Split-phase construction — part 2. Consumes the intermediate state from
    /// [`prepare_resolved`](Self::prepare_resolved): creates the SQLite
    /// database (or wires up the persistent on-disk one), runs each table's
    /// DDL, invokes each file's `on_file` callback, and inserts the
    /// resulting rows.
    ///
    /// Must be invoked on a thread where the `on_file` closures can safely
    /// run. For the napi-rs binding that is the JS main thread.
    #[doc(hidden)]
    pub fn finish_build(prepared: PreparedBuild) -> Result<Self> {
        Self::finish_build_with_fs(prepared, Arc::new(RealFs))
    }

    /// Variant of [`finish_build`] taking the [`FileSystem`] to store on the
    /// instance. Production always passes `Arc::new(RealFs)`; unit tests
    /// inject a fake.
    pub(crate) fn finish_build_with_fs(
        prepared: PreparedBuild,
        fs: Arc<dyn FileSystem>,
    ) -> Result<Self> {
        let PreparedBuild {
            root,
            tables,
            extensions,
            matcher,
            ignore,
            scanned_files,
            persist,
            poll_interval,
            hint_legacy_files_table,
            path_table_parser,
        } = prepared;

        let (mut db, persist_ready) = match persist {
            Some(p) => (p.db, Some((p.deleted, p.meta))),
            None => (Db::new()?, None),
        };
        db.set_path_table_root(root.clone());
        db.set_hint_legacy_files_table(hint_legacy_files_table);
        db.add_path_table_ignore(ignore);
        if let Some(command) = path_table_parser {
            db.set_path_table_parser(command);
        }

        // Load extensions before any CREATE TABLE so a table's DDL and later
        // queries can use extension-provided functions. Loading is enabled
        // only for the duration of each load.
        for ext in &extensions {
            db.load_extension(&ext.path, ext.entrypoint.as_deref())
                .map_err(|source| DirSqlError::Extension {
                    path: ext.path.clone(),
                    source,
                })?;
        }

        let mut on_file_map: HashMap<String, Arc<OnFileFn>> = HashMap::new();
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
            // Reject a `{name}` glob placeholder whose name is also a declared
            // column: captures no longer populate columns, so it would read
            // NULL forever. The table exists here either way (freshly created
            // or restored from cache), so its columns are knowable, and this
            // runs before any file is ingested — a load-time failure.
            let declared_columns = db.get_table_columns(&table_name).map_err(map_db_error)?;
            if let Some(name) = find_capture_column_collision(&table.glob, &declared_columns) {
                return Err(DirSqlError::CaptureColumnCollision {
                    placeholder: name.clone(),
                    column: name,
                });
            }
            on_file_map.insert(table_name.clone(), table.on_file);
            strict_map.insert(table_name.clone(), table.strict);
            ddl_map.insert(table_name, table.ddl);
        }

        // Begin one transaction for the entire ingest: deleted-file cleanup,
        // row deletes/inserts, and meta write. All drop together on any
        // error, leaving the cache exactly as it was before the build started.
        let _tx = db
            .conn()
            .unchecked_transaction()
            .map_err(DirSqlError::sqlite)?;

        // Drop cached rows for files that disappeared since the last cache
        // write. Trusted files need no work: their rows already live in the
        // on-disk SQLite.
        if let Some((deleted, _)) = persist_ready.as_ref() {
            for (rel_path, table_name) in deleted {
                db.delete_rows_by_file(table_name, rel_path)
                    .map_err(map_db_error)?;
                cache_delete_file(db.conn(), rel_path, table_name).map_err(DirSqlError::sqlite)?;
            }
        }

        let snapshot_ns = now_ns();
        let mut on_file_failures: Vec<OnFileFailure> = Vec::new();
        for ScannedFile {
            rel_path,
            table_name,
            stat,
        } in scanned_files
        {
            let on_file = on_file_map.get(&table_name).ok_or_else(|| {
                DirSqlError::Ddl(format!("missing on-file function for table {table_name}"))
            })?;
            let strict = *strict_map.get(&table_name).unwrap_or(&false);
            let abs_path = root.join(&rel_path);
            // A hook failure is this file's problem, not the scan's: record it
            // and keep going, so one unreadable file cannot hide the state of
            // every file after it. The collected failures still fail the build
            // below -- whether a partial index commits is dirsql#697.
            let raw_rows = match on_file(&abs_path.to_string_lossy()) {
                Ok(rows) => rows,
                Err(e) => {
                    on_file_failures.push(OnFileFailure {
                        path: rel_path.clone(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };

            // When updating an existing file in the persistent cache, drop
            // its old rows before inserting the new ones.
            if persist_ready.is_some() {
                db.delete_rows_by_file(&table_name, &rel_path)
                    .map_err(map_db_error)?;
            }
            for (row_index, raw_row) in raw_rows.iter().enumerate() {
                let row = db.normalize_row(&table_name, raw_row, strict)?;
                db.insert_row(&table_name, &row, &rel_path, row_index)
                    .map_err(map_db_error)?;
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
        }

        // Write the meta block last; ingest and meta commit atomically in the
        // single transaction opened above. A crash mid-build leaves the cache
        // exactly as it was before the build started (detected via meta on
        // next startup).
        if let Some((_, meta)) = persist_ready.as_ref() {
            write_meta(db.conn(), meta).map_err(DirSqlError::sqlite)?;
        }

        // Returning before the commit drops `_tx`, so a scan with failures
        // still leaves the cache exactly as it was -- the pre-existing
        // behavior. The single-failure case keeps its original error verbatim;
        // only the multi-failure case reports more than it used to. Whether a
        // partial index should commit instead is dirsql#697.
        match on_file_failures.len() {
            0 => {}
            1 => {
                let only = on_file_failures.remove(0);
                return Err(DirSqlError::OnFile {
                    path: only.path,
                    message: only.message,
                });
            }
            _ => {
                return Err(DirSqlError::OnFileMany {
                    failures: on_file_failures,
                });
            }
        }

        // Commit the ingest transaction.
        _tx.commit().map_err(DirSqlError::sqlite)?;

        // Canonicalize the watch root so the live watcher never sees a
        // relative path; `root` itself is left untouched.
        let watch_root = PathBuf::from(fs.canonical_root(&root));

        Ok(Self {
            inner: Arc::new(DirSqlInner {
                db: Mutex::new(db),
                root,
                watch_root,
                matcher,
                on_file_map,
                strict_map,
                watcher: Mutex::new(None),
                poll_used: AtomicBool::new(false),
                watch_thread_started: AtomicBool::new(false),
                poll_interval,
                fs,
            }),
        })
    }
}

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
/// Pass a `.dirsql.toml` path via [`config`](Self::config). The config file's
/// location does not set the index root: the root is the explicit
/// [`root`](Self::root) when given, else the process cwd.
#[derive(Default)]
pub struct DirSQLBuilder {
    root: Option<PathBuf>,
    tables: Vec<Table>,
    ignore: Vec<String>,
    extensions: Vec<Extension>,
    config_paths: Vec<PathBuf>,
    suppress_config_extensions: bool,
    persist: bool,
    persist_path: Option<PathBuf>,
    poll_interval: Option<Duration>,
    path_table_parser: Option<String>,
}

impl DirSQLBuilder {
    /// Set the root directory to scan. When unset, the index roots at the
    /// process cwd; a config file passed via [`config`](Self::config) never
    /// contributes to root derivation.
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
    /// entries are appended after any programmatic tables and its
    /// `[dirsql].ignore` patterns are appended. The config file does not set the
    /// index root: with no explicit [`root`](Self::root), the index roots at the
    /// process cwd. Relative `persist_path` / `[[dirsql.extension]]` paths still
    /// resolve against the config's parent directory.
    ///
    /// Call repeatedly to load several configs: their `[[table]]`, `ignore`, and
    /// `[[dirsql.extension]]` entries accumulate in call order, and each config's
    /// `on-file` hooks run from that config file's own directory under its own
    /// `[dirsql].hook-timeout`. A single call is identical to before.
    pub fn config(mut self, config_path: impl Into<PathBuf>) -> Self {
        self.config_paths.push(config_path.into());
        self
    }

    /// Suppress loading of a config file's `[[dirsql.extension]]` entries.
    ///
    /// The core resolves config-file extension paths only literally (relative
    /// to the config's parent). A launcher that resolves extensions itself —
    /// e.g. by **package name**, which needs an interpreter the compiled core
    /// lacks (Python `importlib`, Node `require.resolve`) — sets this and
    /// supplies the already-resolved literal paths via
    /// [`extensions`](Self::extensions) instead, so the config's own extension
    /// entries are not loaded a second time.
    pub fn suppress_config_extensions(mut self, suppress: bool) -> Self {
        self.suppress_config_extensions = suppress;
        self
    }

    /// Enable persistent on-disk storage. `None` writes the SQLite database to
    /// the default `<root>/.dirsql/cache.db`; `Some(path)` writes it to `path`.
    /// Either way, subsequent startups only re-parse files that have actually
    /// changed. See `docs/howto/persist.md` for the reconcile contract.
    pub fn persist(mut self, path: Option<impl AsRef<Path>>) -> Self {
        self.persist = true;
        if let Some(path) = path {
            self.persist_path = Some(path.as_ref().to_path_buf());
        }
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

    /// Attach a parser command to every path-table this index mints (the CLI's
    /// `--on-file`): a path-table's rows and schema then come from the command's
    /// output instead of the stat columns. Internal plumbing for the CLI flag —
    /// no config-file or named-table interaction.
    #[doc(hidden)]
    pub fn path_table_parser(mut self, command: impl Into<String>) -> Self {
        self.path_table_parser = Some(command.into());
        self
    }

    fn resolve(self) -> Result<ResolvedBuild> {
        let DirSQLBuilder {
            root: explicit_root,
            mut tables,
            mut ignore,
            mut extensions,
            config_paths,
            suppress_config_extensions,
            persist,
            persist_path,
            poll_interval,
            path_table_parser,
        } = self;

        // The index root is an operational fact owned by the runner: the
        // explicit `.root(...)` when given, else the process cwd. The config
        // file's own location plays no part in root derivation (#540).
        let root = match explicit_root {
            Some(explicit) => explicit,
            None => std::env::current_dir().map_err(DirSqlError::config)?,
        };

        // The config layer is an ordered list of entries, each carrying its
        // loaded `config`, its `config_dir` (where `on-file` hooks run and the
        // base for extension path resolution), and its `hook_timeout`. Configs
        // accumulate in `.config()` call order; a single entry makes the in-order
        // merge below byte-for-byte identical to a single pass.
        let mut config_entries: Vec<ResolvedConfigEntry> = Vec::new();
        for cfg_path in &config_paths {
            let cfg = config::load_config(cfg_path).map_err(DirSqlError::config)?;

            let cfg_parent = cfg_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let hook_timeout = cfg.hook_timeout.unwrap_or(command::DEFAULT_COMMAND_TIMEOUT);
            config_entries.push(ResolvedConfigEntry {
                config: cfg,
                config_dir: cfg_parent,
                hook_timeout,
            });
        }

        for entry in config_entries {
            let ResolvedConfigEntry {
                config: cfg,
                config_dir: cfg_parent,
                hook_timeout,
            } = entry;

            // `on-file` commands run in the config file's directory; `{path}`
            // is the matched file's absolute path and `{root}` the resolved
            // index root.
            let cfg_tables = build_tables_from_config(&cfg, &cfg_parent, &root, hook_timeout)?;
            tables.extend(cfg_tables);
            ignore.extend(cfg.ignore);

            // Config-supplied extension paths resolve against the config
            // file's parent directory (absolute paths pass through); see
            // `suppress_config_extensions` for the skip.
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
        }

        let hint_legacy_files_table = is_configless(&config_paths, &tables);

        Ok(ResolvedBuild {
            root,
            tables,
            ignore,
            extensions,
            persist,
            persist_path,
            poll_interval: poll_interval.unwrap_or(DEFAULT_POLL_INTERVAL),
            hint_legacy_files_table,
            path_table_parser,
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

/// Default poll interval for the channel-based watch loop.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// One resolved config file in the ordered list [`DirSQLBuilder::resolve`]
/// merges over. Carries the loaded `config`, its `config_dir` (the config
/// file's parent -- where `on-file` hooks run and the base for extension path
/// resolution), and the `hook_timeout` bounding each `on-file` run. The index
/// root is owned by the runner (#540), not derived per config entry.
struct ResolvedConfigEntry {
    config: config::Config,
    config_dir: PathBuf,
    hook_timeout: Duration,
}

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
    /// Arms the missing-`files` path-table hint; see [`is_configless`].
    pub hint_legacy_files_table: bool,
    /// When set (the CLI's `--on-file`), every path-table is minted over this
    /// parser command instead of the stat columns.
    pub path_table_parser: Option<String>,
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
    /// The configured skip rules, carried through so path-table scans apply
    /// the same ones declared tables do.
    ignore: Vec<String>,
    scanned_files: Vec<ScannedFile>,
    persist: Option<PreparedPersist>,
    poll_interval: Duration,
    hint_legacy_files_table: bool,
    path_table_parser: Option<String>,
}

#[doc(hidden)]
pub struct PreparedPersist {
    db: Db,
    deleted: Vec<(String, String)>,
    meta: HashMap<String, String>,
}

/// A file the reconcile decided to trust: its cached rows are kept as-is and
/// the file is not re-parsed.
#[doc(hidden)]
pub struct TrustedFile {
    pub rel_path: String,
    pub table_name: String,
}

struct PersistContext {
    db: Db,
    /// Cached file bookkeeping keyed by `(rel_path, table_name)` — a file may
    /// be cached under several tables under fan-out.
    cached: HashMap<(String, String), CachedFile>,
    expected_meta: HashMap<String, String>,
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
        // would-be-injection DDL can't propagate into `on_file_map`,
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
/// wiped and the resulting [`PersistContext`] carries an empty file index,
/// so the rest of the pipeline treats every file as new.
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

    let cached = if compatible {
        read_cached_files(db.conn()).map_err(DirSqlError::sqlite)?
    } else {
        drop_user_tables(db.conn()).map_err(DirSqlError::sqlite)?;
        HashMap::new()
    };

    Ok(PersistContext {
        db,
        cached,
        expected_meta,
    })
}

/// Internal filesystem seam: every effectful filesystem read in the
/// persist/reconcile and watch-upsert paths goes through this trait so unit
/// tests can inject a deterministic double. Production always uses [`RealFs`].
trait FileSystem: Send + Sync {
    fn stat(&self, path: &Path) -> std::io::Result<FileStat>;
    /// Whether `path` is a regular file. `Err(NotFound)` when it doesn't exist.
    fn is_file(&self, path: &Path) -> std::io::Result<bool>;
    /// BLAKE3-hash a file's contents.
    fn hash(&self, path: &Path) -> std::io::Result<[u8; 32]>;
    /// Canonicalize the watch root, falling back to the literal path.
    fn canonical_root(&self, root: &Path) -> String;
}

struct RealFs;

impl FileSystem for RealFs {
    fn stat(&self, path: &Path) -> std::io::Result<FileStat> {
        std::fs::metadata(path).map(|m| FileStat::from_metadata(&m))
    }

    fn is_file(&self, path: &Path) -> std::io::Result<bool> {
        std::fs::metadata(path).map(|m| m.is_file())
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
    // Keyed by (rel_path, table_name): under fan-out one file may be scanned
    // for several tables, and each pair is trusted/deleted independently.
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::with_capacity(scanned.len());

    for (path, table_name) in scanned {
        let rel_path = relative_path(root, &path);
        seen.insert((rel_path.clone(), table_name.clone()));

        let stat = fs.stat(&path)?;

        let cached = ctx.cached.get(&(rel_path.clone(), table_name.clone()));
        let trust = match cached {
            Some(c) if c.stat == stat => {
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
    for (rel_path, table_name) in ctx.cached.keys() {
        if !seen.contains(&(rel_path.clone(), table_name.clone())) {
            deleted.push((rel_path.clone(), table_name.clone()));
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

/// Build [`Table`] objects from a parsed config.
///
/// A config-defined table runs its `on-file` command once per matched file
/// (see [`run_on_file`]): the command reads the file and prints a JSON array of
/// row objects on stdout, which becomes the file's rows verbatim. The core
/// injects nothing — a DDL column the hook does not emit is NULL, validated
/// against the DDL as usual. `config_dir` is the command's working directory
/// (the config file's parent) and `root` is the resolved index root exposed as
/// the `{root}` placeholder. `timeout` bounds each `on-file` run; the caller
/// resolves it from the global `[dirsql].hook-timeout` key, falling back to
/// [`command::DEFAULT_COMMAND_TIMEOUT`].
/// Whether this build declared no tables by any route — neither a config file
/// nor a programmatic table. That is exactly the state in which `files` used to
/// exist implicitly, and so the only state whose missing-`files` error earns the
/// path-table hint. Pure so the whole truth table is unit-testable without I/O.
fn is_configless(config_paths: &[PathBuf], tables: &[Table]) -> bool {
    config_paths.is_empty() && tables.is_empty()
}

fn build_tables_from_config(
    cfg: &config::Config,
    config_dir: &Path,
    root: &Path,
    timeout: Duration,
) -> Result<Vec<Table>> {
    let mut tables = Vec::with_capacity(cfg.tables.len());

    for table_cfg in &cfg.tables {
        let command = table_cfg.on_file.clone();
        let config_dir = config_dir.to_path_buf();
        let root = root.to_path_buf();
        // `Table::new` (infallible): `run_on_file` isolates its own errors to an
        // empty row set so one bad file never aborts the scan (the scan aborts
        // on an on_file `Err`).
        let mut table = Table::new(
            table_cfg.ddl.clone(),
            table_cfg.glob.clone(),
            move |abs_path: &str| run_on_file(&command, abs_path, &config_dir, &root, timeout),
        );

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
/// Placeholders: `{path}` (the file's absolute path) and `{root}` (the index
/// root). An absolute `{path}` is self-sufficient from any cwd, so a hook whose
/// config lives outside the index still resolves it. A template that omits a
/// placeholder simply never receives its value — nothing is appended.
///
/// Per-file isolation: any failure — a spawn/exit/timeout error from
/// [`command::run_command`], or output that is not a JSON array of objects —
/// is logged to stderr and yields no rows (`vec![]`). Returning `Err` here
/// would abort the whole scan, so it never does. `timeout` bounds the run —
/// the table's configured value, or the shared default.
fn run_on_file(
    command: &str,
    abs_path: &str,
    config_dir: &Path,
    root: &Path,
    timeout: Duration,
) -> Vec<Row> {
    let placeholders = [
        Placeholder::new("path", abs_path),
        Placeholder::new("root", root.to_string_lossy().into_owned()),
    ];

    match command::run_command(command, &placeholders, config_dir, timeout, None) {
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
/// any element is not an object.
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
/// an array/object → its JSON text as `Text`.
pub(crate) fn json_to_value(value: &serde_json::Value) -> Value {
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
const STAT_PATH: &str = "path";
const STAT_BASENAME: &str = "basename";
const STAT_DIR: &str = "dir";
const STAT_EXT: &str = "ext";
const STAT_SIZE: &str = "size";
const STAT_MTIME: &str = "mtime";
const STAT_CTIME: &str = "ctime";

/// Compute the filesystem-fact columns for a given file: path-derived
/// (`path`, `basename`, `dir`, `ext`) and stat-derived (`size`,
/// `mtime`, `ctime`).
pub(crate) fn compute_stat_virtuals(rel_path: &str, abs_path: &Path) -> Row {
    // A missing/unreadable file yields all-`None` (absent columns);
    // `mtime`/`ctime` are `None` when the platform can't supply them or the
    // value predates the epoch.
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
/// corresponding fact is unavailable).
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
        // `Photo.JPG` and `photo.jpg` are distinct files.
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

/// Returns the first `{name}` placeholder in `glob` whose name is also one of
/// `declared_columns`. `None` when the glob has no placeholders or none of
/// them names a declared column. Pure: the sole input is the glob string and
/// the column list, so it is exhaustively unit-testable.
fn find_capture_column_collision(glob: &str, declared_columns: &[String]) -> Option<String> {
    let declared: std::collections::HashSet<&str> =
        declared_columns.iter().map(String::as_str).collect();
    crate::matcher::placeholder_names(glob)
        .into_iter()
        .find(|name| declared.contains(name.as_str()))
}

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

#[cfg(test)]
impl AsyncDirSqlInner {
    /// Fresh inner with an empty `db` cell — the pre-`ready` state.
    fn empty() -> Self {
        Self {
            db: tokio::sync::OnceCell::new(),
            ready_notify: tokio::sync::Notify::new(),
        }
    }
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

    /// Shortcut for `DirSQL::builder().config(config_path).build_async()`.
    ///
    /// With no explicit `.root()`, the index roots at the process cwd, not the
    /// config file's parent directory. To read `<root>/.dirsql.toml`, pass it
    /// explicitly: `AsyncDirSQL::from_config_path(root.join(".dirsql.toml"))`
    /// (the implicit root-joining `from_config(root)` shortcut was removed in
    /// #603).
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
        assert!(matches!(err, DirSqlError::WriteForbidden), "got: {err:?}");
    }

    #[test]
    fn map_db_error_leaves_schema_mismatch_as_core() {
        let err = map_db_error(DbError::SchemaMismatch("nope".into()));
        assert!(matches!(err, DirSqlError::Core(_)), "got: {err:?}");
    }

    #[test]
    fn missing_extension_build_fails_with_extension_error() {
        // A missing extension file must surface as DirSqlError::Extension
        // (naming the library), not the generic Core(Sqlite) error.
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
        assert_eq!(
            DirSqlError::lock("x").to_string(),
            "failed to lock shared state: x"
        );
        // `watch`, `config`, `matcher` wrap a typed StdError to preserve a
        // `source()` chain.
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
    use std::collections::HashSet as StdHashSet;
    use tempfile::TempDir;

    /// Deterministic [`FileSystem`] double: canned [`FileStat`]s and hashes;
    /// any path not present stats/hashes as `NotFound`. A path registered via
    /// [`with_dir`](FakeFs::with_dir) reports as a non-file; any other stat'd
    /// path reports as a regular file.
    #[derive(Default)]
    struct FakeFs {
        stats: StdHashMap<PathBuf, FileStat>,
        hashes: StdHashMap<PathBuf, [u8; 32]>,
        canonical_roots: StdHashMap<PathBuf, String>,
        dirs: StdHashSet<PathBuf>,
    }

    impl FakeFs {
        fn with_stat(path: impl Into<PathBuf>, stat: FileStat) -> Self {
            let mut fs = FakeFs::default();
            fs.stats.insert(path.into(), stat);
            fs
        }

        /// Register `path` as an existing non-file (a directory).
        fn with_dir(mut self, path: impl Into<PathBuf>) -> Self {
            self.dirs.insert(path.into());
            self
        }

        fn set_hash(&mut self, path: impl Into<PathBuf>, hash: [u8; 32]) {
            self.hashes.insert(path.into(), hash);
        }

        /// Register a canned canonicalization for `root`.
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

        fn is_file(&self, path: &Path) -> std::io::Result<bool> {
            if self.dirs.contains(path) {
                return Ok(false);
            }
            if self.stats.contains_key(path) {
                return Ok(true);
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "fake: no such file",
            ))
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

    /// A canned [`FileStat`] for tests that only need a stat to succeed.
    fn fake_stat() -> FileStat {
        FileStat {
            size: 5,
            mtime_ns: 1_000,
            ctime_ns: 1_000,
            inode: 1,
            dev: 1,
        }
    }

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

    #[test]
    fn compute_stat_virtuals_skips_absent_fields() {
        let stat = compute_stat_virtuals("bare", Path::new("/nonexistent-xyz/bare"));
        assert_eq!(stat[STAT_PATH], Value::Text("bare".into()));
        assert_eq!(stat[STAT_BASENAME], Value::Text("bare".into()));
        // `Path::new("bare").parent()` is `Some("")`, so `dir` is an empty
        // string rather than absent; there is no extension and no metadata.
        assert!(!stat.contains_key(STAT_EXT));
        assert!(!stat.contains_key(STAT_SIZE));
        assert!(!stat.contains_key(STAT_MTIME));
        assert!(!stat.contains_key(STAT_CTIME));
    }

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

    /// A `ScannedFile` whose table has no registered on_file function must
    /// error rather than be silently skipped.
    #[test]
    fn finish_build_errors_on_ghost_scanned_file() {
        let dir = TempDir::new().unwrap();
        let matcher = TableMatcher::new(&[], &[]).unwrap();
        let prepared = PreparedBuild {
            ignore: Vec::new(),
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
            hint_legacy_files_table: false,
            path_table_parser: None,
        };
        assert!(DirSQL::finish_build(prepared).is_err());
    }

    #[test]
    fn process_file_event_skips_ignored_paths() {
        let dir = TempDir::new().unwrap();
        let kept = dir.path().join("keep.txt");
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

        let ignored = dir.path().join("skip").join("a.txt");
        let events = db.process_file_event(FileEvent::Created(ignored));
        assert!(events.is_empty(), "ignored path must produce no events");

        let events = db.process_file_event(FileEvent::Created(kept));
        assert_eq!(events.len(), 1, "non-ignored path must produce one event");
    }

    /// Building with a **relative** root canonicalizes `watch_root` to an
    /// absolute path while leaving `root` exactly as the caller supplied it,
    /// so `notify` never sees `.`.
    #[test]
    fn relative_root_canonicalizes_watch_root_only() {
        let fake = FakeFs::default().with_canonical_root(".", "/ws/canonical");
        let db = DirSQL::with_ignore_and_fs(
            ".",
            vec![Table::new("CREATE TABLE t (x TEXT)", "*.txt", |_| vec![])],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        assert_eq!(db.inner.root, PathBuf::from("."));
        assert!(
            db.inner.watch_root.is_absolute(),
            "watch_root must be absolute, got {:?}",
            db.inner.watch_root
        );
        assert_eq!(db.inner.watch_root, PathBuf::from("/ws/canonical"));
    }

    /// With an absolute root, `process_file_event` strips the `watch_root`
    /// prefix to yield a root-relative `path`.
    #[test]
    fn process_file_event_strips_watch_root_prefix() {
        let root = PathBuf::from("/ws");
        let abs = root.join("nested").join("a.txt");
        let fake = FakeFs::with_stat(abs.clone(), fake_stat()).with_canonical_root(&root, "/ws");
        let db = DirSQL::with_ignore_and_fs(
            &root,
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

        let events = db.process_file_event(FileEvent::Created(abs));
        assert_eq!(events.len(), 1, "expected one insert: {events:?}");
        match &events[0] {
            RowEvent::Insert { file_path, .. } => {
                assert_eq!(
                    file_path, "nested/a.txt",
                    "watch_root prefix must be stripped to a root-relative path"
                );
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    /// When an event path lies under the user-supplied `root` but not under
    /// the canonical `watch_root`, the fallback strips `root` instead.
    #[test]
    fn process_file_event_falls_back_to_root_prefix() {
        let root = PathBuf::from("/ws");
        let abs = root.join("b.txt");
        let fake = FakeFs::with_stat(abs.clone(), fake_stat()).with_canonical_root(&root, "/ws");
        let mut db = DirSQL::with_ignore_and_fs(
            &root,
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

        // Repoint watch_root to a non-prefix sibling so the first strip misses.
        Arc::get_mut(&mut db.inner).unwrap().watch_root = root.join("does-not-prefix");

        let events = db.process_file_event(FileEvent::Created(abs));
        assert_eq!(events.len(), 1, "expected one insert: {events:?}");
        match &events[0] {
            RowEvent::Insert { file_path, .. } => {
                assert_eq!(
                    file_path, "b.txt",
                    "root fallback must strip the user-supplied root prefix"
                );
            }
            other => panic!("expected Insert, got {other:?}"),
        }
    }

    /// When the event path is under neither `watch_root` nor `root`, the
    /// absolute path is kept as the relative path.
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

        // The outside path matches no table glob, so no events are produced;
        // the strip fallback still runs.
        let outside = PathBuf::from("/some/elsewhere/c.md");
        let events = db.process_file_event(FileEvent::Created(outside));
        assert!(
            events.is_empty(),
            "unmatched absolute path must produce no events: {events:?}"
        );
    }

    /// A cached file whose `snapshot_ns <= mtime_ns` is inside the racy
    /// window; a matching content hash confirms it and the file is trusted.
    #[test]
    fn reconcile_scan_hash_confirms_in_racy_window() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let stat = fake_stat();
        let live_hash = [7u8; 32];
        let mut fake = FakeFs::with_stat(abs.clone(), stat.clone());
        fake.set_hash(abs.clone(), live_hash);

        let mut cached = HashMap::new();
        cached.insert(
            ("a.txt".to_string(), "t".to_string()),
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
        };
        let scanned = vec![(abs.clone(), "t".to_string())];
        let (to_parse, trusted, deleted) =
            reconcile_scan(dir.path(), scanned, &ctx, &fake).unwrap();
        assert!(to_parse.is_empty());
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].rel_path, "a.txt");
        assert!(deleted.is_empty());
    }

    /// Same racy-window entry but with no stored content hash: the file
    /// cannot be confirmed, so it is re-parsed.
    #[test]
    fn reconcile_scan_racy_window_without_hash_reparses() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("b.txt");
        let stat = fake_stat();
        let mut fake = FakeFs::with_stat(abs.clone(), stat.clone());
        fake.set_hash(abs.clone(), [9u8; 32]);

        let mut cached = HashMap::new();
        cached.insert(
            ("b.txt".to_string(), "t".to_string()),
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
        };
        let scanned = vec![(abs.clone(), "t".to_string())];
        let (to_parse, trusted, _deleted) =
            reconcile_scan(dir.path(), scanned, &ctx, &fake).unwrap();
        assert_eq!(to_parse.len(), 1);
        assert!(trusted.is_empty());
    }

    #[test]
    fn reconcile_scan_errors_when_file_vanished() {
        let dir = TempDir::new().unwrap();
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached: HashMap::new(),
            expected_meta: HashMap::new(),
        };
        let missing = dir.path().join("ghost.txt");
        let scanned = vec![(missing, "t".to_string())];
        let fake = FakeFs::default();
        assert!(reconcile_scan(dir.path(), scanned, &ctx, &fake).is_err());
    }

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

        assert!(
            !fs.is_file(dir.path()).unwrap(),
            "a directory must report is_file=false"
        );
        assert!(
            fs.is_file(&missing).is_err(),
            "is_file of a missing path must error"
        );
        // A nonexistent path can't canonicalize, so the literal fallback runs.
        assert_eq!(
            fs.canonical_root(&missing),
            missing.to_string_lossy(),
            "canonical_root must fall back to the literal path when it can't canonicalize"
        );
    }

    /// Poison a mutex by panicking while holding its guard: the guard's
    /// `Drop` runs during unwinding and marks the mutex poisoned.
    fn poison<T: Send>(m: &Mutex<T>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("poison");
        }));
        assert!(m.is_poisoned(), "mutex should be poisoned");
    }

    /// Build a `DirSQL` over an *empty* temp dir with no explicit tables, so
    /// the injected baked-in `files` table (#603) has zero rows -- effectively
    /// tableless for these lock-poison / error-path tests.
    fn simple_db() -> (TempDir, DirSQL) {
        let dir = TempDir::new().unwrap();
        let db =
            DirSQL::with_ignore(dir.path(), Vec::<Table>::new(), Vec::<String>::new()).unwrap();
        (dir, db)
    }

    /// Build a one-table `DirSQL` whose fake fs stats `a.txt` successfully
    /// (any other path stats as NotFound). Returns the db plus the file's
    /// absolute and relative paths so callers can drive `handle_*` directly.
    fn upsert_fixture() -> (TempDir, DirSQL, PathBuf, String) {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
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

    /// Assert one `RowEvent::Error` whose message reports a poisoned lock.
    fn assert_single_lock_error(events: &[RowEvent]) {
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("poisoned lock"), "expected poison text: {dbg}");
    }

    #[test]
    fn handle_delete_surfaces_db_poison() {
        let (_dir, db, _abs, rel) = upsert_fixture();
        poison(&db.inner.db);
        let events = db.handle_delete("items", &rel);
        assert_single_lock_error(&events);
    }

    #[test]
    fn handle_delete_surfaces_db_failure() {
        let (_dir, db, _abs, _rel) = upsert_fixture();
        // No SQL table named `ghost` exists, so the old-row snapshot fails.
        let events = db.handle_delete("ghost", "whatever.txt");
        assert_eq!(events.len(), 1, "expected one error event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("no such table"), "expected a SQL error: {dbg}");
    }

    #[test]
    fn handle_delete_surfaces_delete_failure() {
        // The old-row snapshot succeeds but the delete itself fails: a
        // trigger aborts every DELETE on items.
        let (_dir, db, abs, rel) = upsert_fixture();
        let events = db.handle_upsert("items", &abs, &rel);
        assert_eq!(events.len(), 1, "fixture insert failed: {events:?}");
        {
            let guard = db.inner.db.lock().unwrap();
            guard
                .conn()
                .execute(
                    "CREATE TRIGGER items_no_delete BEFORE DELETE ON items \
                     BEGIN SELECT RAISE(ABORT, 'delete forbidden by test trigger'); END",
                    [],
                )
                .unwrap();
        }
        let events = db.handle_delete("items", &rel);
        assert_eq!(events.len(), 1, "expected one error event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("delete forbidden"), "got: {dbg}");
    }

    #[test]
    fn handle_upsert_surfaces_db_poison() {
        let (_dir, db, abs, rel) = upsert_fixture();
        poison(&db.inner.db);
        let events = db.handle_upsert("items", &abs, &rel);
        assert_single_lock_error(&events);
    }

    #[test]
    fn handle_upsert_surfaces_insert_failure() {
        // Normalize and the old-row snapshot succeed, but the write-back
        // fails: a trigger aborts every INSERT on items.
        let (_dir, db, abs, rel) = upsert_fixture();
        {
            let guard = db.inner.db.lock().unwrap();
            guard
                .conn()
                .execute(
                    "CREATE TRIGGER items_no_insert BEFORE INSERT ON items \
                     BEGIN SELECT RAISE(ABORT, 'insert forbidden by test trigger'); END",
                    [],
                )
                .unwrap();
        }
        let events = db.handle_upsert("items", &abs, &rel);
        assert_eq!(events.len(), 1, "expected one error event: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("insert forbidden"), "got: {dbg}");
    }

    #[test]
    fn handle_upsert_skips_directory() {
        // A `mkdir` under the root matches a `**/*` glob, but a directory must
        // not become a row — mirror the initial scan's non-file skip.
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("subdir");
        let fake = FakeFs::default().with_dir(subdir.clone());
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::new("CREATE TABLE files (name TEXT)", "**/*", |_| {
                vec![Row::from_iter([(
                    "name".to_string(),
                    Value::Text("x".into()),
                )])]
            })],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        let events = db.handle_upsert("files", &subdir, "subdir");
        assert!(events.is_empty(), "a directory must not produce row events");
        assert!(
            db.query("SELECT * FROM files").unwrap().is_empty(),
            "a directory must not insert a row"
        );
    }

    #[test]
    fn handle_upsert_returns_empty_when_file_vanished() {
        let (dir, db, _abs, _rel) = upsert_fixture();
        let missing = dir.path().join("gone.txt");
        let events = db.handle_upsert("items", &missing, "gone.txt");
        assert!(events.is_empty(), "vanished file must produce no events");
    }

    #[test]
    fn handle_upsert_returns_empty_for_unknown_table() {
        let (_dir, db, abs, rel) = upsert_fixture();
        let events = db.handle_upsert("not_a_table", &abs, &rel);
        assert!(events.is_empty(), "unknown table must produce no events");
    }

    #[test]
    fn handle_upsert_surfaces_normalize_error_in_strict_mode() {
        // The on_file emits an undeclared `extra` column, so the strict-mode
        // normalize rejects it with a SchemaMismatch.
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
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

    #[test]
    fn run_channel_loop_emits_error_event_on_poll_failure() {
        let (_dir, db) = simple_db();
        // Poison the started watcher so the loop's first `poll_once` errors.
        db.start_watching().unwrap();
        poison(&db.inner.watcher);

        let (tx, mut rx) = unbounded();
        run_channel_loop(db, tx);

        let event = rx.try_recv().expect("expected an error event");
        let dbg = format!("{event:?}");
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("failed to lock"), "expected lock text: {dbg}");
        assert!(rx.try_recv().is_err(), "loop should have ended");
    }

    #[test]
    fn is_configless_only_when_config_and_tables_are_both_empty() {
        // Pure truth table (no I/O): the missing-`files` hint is armed only
        // when neither a config path nor a programmatic table was supplied.
        let table = Table::new("CREATE TABLE x (a TEXT)", "*", |_| vec![Row::new()]);
        assert!(is_configless(&[], &[]));
        assert!(!is_configless(&[PathBuf::from("c.toml")], &[]));
        assert!(!is_configless(&[], std::slice::from_ref(&table)));
        assert!(!is_configless(
            &[PathBuf::from("c.toml")],
            std::slice::from_ref(&table)
        ));
    }

    #[test]
    fn resolve_with_no_config_or_tables_yields_no_tables_and_arms_the_hint() {
        // With no config and no programmatic tables the builder defines no
        // named tables at all; path-tables serve filesystem queries. The
        // missing-`files` hint is armed for exactly this state.
        let resolved = DirSQL::builder().root("/tmp/x").resolve().unwrap();
        assert!(
            resolved.tables.is_empty(),
            "expected no tables, got {:?}",
            resolved.tables.len()
        );
        assert!(resolved.hint_legacy_files_table);
    }

    #[test]
    fn path_table_parser_defaults_to_none() {
        let resolved = DirSQL::builder().root("/tmp/x").resolve().unwrap();
        assert!(resolved.path_table_parser.is_none());
    }

    #[test]
    fn path_table_parser_carries_the_command_through_resolve() {
        let resolved = DirSQL::builder()
            .root("/tmp/x")
            .path_table_parser("parse.py {path}")
            .resolve()
            .unwrap();
        assert_eq!(
            resolved.path_table_parser.as_deref(),
            Some("parse.py {path}")
        );
    }

    #[test]
    fn resolve_with_a_programmatic_table_disarms_the_hint() {
        let with_table = DirSQL::builder()
            .root("/tmp/x")
            .table(Table::new("CREATE TABLE t (a TEXT)", "*.t", |_| {
                vec![Row::new()]
            }))
            .resolve()
            .unwrap();
        assert_eq!(with_table.tables.len(), 1);
        assert!(with_table.tables[0].ddl.starts_with("CREATE TABLE t"));
        assert!(
            !with_table.hint_legacy_files_table,
            "a user who declared tables gets the plain error"
        );
    }

    #[test]
    fn from_config_path_errors_when_config_missing() {
        // `from_config_path` (the explicit constructor that stays) errors when
        // the named file does not exist -- no silent fallback to the default.
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.toml");
        assert!(DirSQL::from_config_path(&missing).is_err());
    }

    #[test]
    fn resolve_without_root_defaults_to_process_cwd() {
        // No `.root()`, no `.config()`: the index root falls back to the
        // process cwd, which is always an absolute path. The exact-cwd
        // assertion lives in the `config_root_derivation` integration test,
        // which may mutate the process cwd (forbidden in a unit test).
        let resolved = DirSQL::builder().resolve().unwrap();
        assert!(resolved.root.is_absolute());
    }

    #[test]
    fn resolve_explicit_root_is_used_verbatim() {
        let resolved = DirSQL::builder()
            .root("/some/explicit/root")
            .resolve()
            .unwrap();
        assert_eq!(resolved.root, PathBuf::from("/some/explicit/root"));
    }

    #[test]
    fn duplicate_table_names_are_rejected() {
        let dir = TempDir::new().unwrap();
        let err = match DirSQL::new(
            dir.path(),
            vec![
                Table::new("CREATE TABLE t (a TEXT)", "*.a", |_| vec![]),
                Table::new("CREATE TABLE t (b TEXT)", "*.b", |_| vec![]),
            ],
        ) {
            Ok(_) => panic!("expected a duplicate-table error"),
            Err(e) => e,
        };
        assert!(
            matches!(err, DirSqlError::DuplicateTable(ref n) if n == "t"),
            "got: {err:?}"
        );
    }

    #[test]
    fn start_watching_is_idempotent() {
        let (_dir, db) = simple_db();
        db.start_watching().unwrap();
        db.start_watching().unwrap();
    }

    #[test]
    fn poll_events_returns_empty_without_activity() {
        let (_dir, db) = simple_db();
        assert!(db.poll_events(Duration::from_millis(0)).unwrap().is_empty());
    }

    #[test]
    fn wait_file_events_returns_empty_batch_without_activity() {
        let (_dir, db) = simple_db();
        assert!(
            db.wait_file_events(Duration::from_millis(0))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn watch_locks_out_the_poll_apis() {
        let (_dir, db) = simple_db();
        let _stream = db.watch().unwrap();
        let e1 = db.poll_events(Duration::from_millis(0)).unwrap_err();
        assert!(e1.to_string().contains("watch() is active"), "got: {e1}");
        let e2 = db.wait_file_events(Duration::from_millis(0)).unwrap_err();
        assert!(e2.to_string().contains("watch() is active"), "got: {e2}");
    }

    #[test]
    fn poll_events_locks_out_watch() {
        let (_dir, db) = simple_db();
        db.poll_events(Duration::from_millis(0)).unwrap();
        let err = db.watch().unwrap_err();
        assert!(
            err.to_string().contains("poll_events() already in use"),
            "got: {err}"
        );
    }

    #[test]
    fn watch_twice_reports_already_started() {
        let (_dir, db) = simple_db();
        let _stream = db.watch().unwrap();
        let err = db.watch().unwrap_err();
        assert!(
            matches!(err, DirSqlError::WatchAlreadyStarted),
            "got: {err:?}"
        );
    }

    #[test]
    fn apply_file_events_processes_create_then_delete() {
        let (_dir, db, abs, _rel) = upsert_fixture();
        let created = db.apply_file_events(vec![FileEvent::Created(abs.clone())]);
        assert_eq!(created.len(), 1, "expected one insert: {created:?}");
        assert!(matches!(&created[0], RowEvent::Insert { .. }));

        let deleted = db.apply_file_events(vec![FileEvent::Deleted(abs)]);
        assert_eq!(deleted.len(), 1, "expected one delete: {deleted:?}");
        assert!(matches!(&deleted[0], RowEvent::Delete { .. }));
    }

    #[test]
    fn handle_upsert_inserts_and_diffs_rows() {
        let (_dir, db, abs, rel) = upsert_fixture();
        let events = db.handle_upsert("items", &abs, &rel);
        assert_eq!(events.len(), 1, "got: {events:?}");
        assert!(matches!(&events[0], RowEvent::Insert { .. }));

        let del = db.handle_delete("items", &rel);
        assert_eq!(del.len(), 1, "got: {del:?}");
        assert!(matches!(&del[0], RowEvent::Delete { .. }));
    }

    #[test]
    fn handle_upsert_surfaces_on_file_error() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let fake = FakeFs::with_stat(abs.clone(), fake_stat());
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::try_new(
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| Err("boom".into()),
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();
        let events = db.handle_upsert("items", &abs, "a.txt");
        assert_eq!(events.len(), 1, "got: {events:?}");
        let dbg = format!("{:?}", events[0]);
        assert!(dbg.contains("Error"), "got: {dbg}");
        assert!(dbg.contains("boom"), "got: {dbg}");
    }

    #[test]
    fn builder_setters_configure_and_build() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("custom-cache.db");
        let db = DirSQL::builder()
            .root(dir.path())
            .tables(vec![Table::new(
                "CREATE TABLE b (y TEXT)",
                "*.b",
                |_| vec![],
            )])
            .table(Table::new("CREATE TABLE a (x TEXT)", "*.a", |_| vec![]))
            .ignore(["skip/**"])
            .extensions(Vec::<Extension>::new())
            .suppress_config_extensions(true)
            .persist(Some(&cache))
            .poll_interval(Duration::from_millis(50))
            .build()
            .unwrap();
        assert!(db.query("SELECT * FROM a").is_ok());
        assert!(db.query("SELECT * FROM b").is_ok());
        assert_eq!(db.inner.poll_interval, Duration::from_millis(50));
    }

    #[test]
    fn build_wires_the_index_root_as_the_path_table_root() {
        let dir = TempDir::new().unwrap();
        let db = DirSQL::builder().root(dir.path()).build().unwrap();

        assert!(
            db.query("SELECT path FROM './'").is_ok(),
            "the path-table fallback must be armed on an ephemeral db"
        );
    }

    #[test]
    fn build_wires_the_path_table_parser_onto_the_index() {
        // Covers the `Some(command)` arm of finish_build: the parser is stored
        // at build time and path-tables are minted over the parsed module. Over
        // an empty root, a parsed path-table matches no files, so schema
        // inference has no sample and the query reports the parsed-mode
        // "no rows" error — proof the parser branch is armed, without writing
        // any file (which the unit-isolation gate bars) or spawning the parser.
        let dir = TempDir::new().unwrap();
        let db = DirSQL::builder()
            .root(dir.path())
            .path_table_parser("cat {path}")
            .build()
            .unwrap();

        let err = db
            .query("SELECT x FROM './*.json'")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no rows"),
            "a parsed path-table over an empty root reports the no-rows error; got: {err}"
        );
    }

    #[test]
    fn a_persisted_build_also_wires_the_path_table_root() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache.db");
        let db = DirSQL::builder()
            .root(dir.path())
            .persist(Some(&cache))
            .build()
            .unwrap();

        assert!(
            db.query("SELECT path FROM './'").is_ok(),
            "the persist branch must arm the fallback too"
        );
    }

    #[test]
    fn persist_none_enables_default_path() {
        let resolved = DirSQL::builder()
            .root("/tmp/x")
            .persist(None::<&Path>)
            .resolve()
            .unwrap();
        assert!(resolved.persist);
        assert!(resolved.persist_path.is_none());
    }

    #[test]
    fn persist_some_enables_explicit_path() {
        let resolved = DirSQL::builder()
            .root("/tmp/x")
            .persist(Some("/tmp/x/custom.db"))
            .resolve()
            .unwrap();
        assert!(resolved.persist);
        assert_eq!(
            resolved.persist_path,
            Some(PathBuf::from("/tmp/x/custom.db"))
        );
    }

    #[test]
    fn persist_unset_leaves_persistence_off() {
        let resolved = DirSQL::builder().root("/tmp/x").resolve().unwrap();
        assert!(!resolved.persist);
        assert!(resolved.persist_path.is_none());
    }

    /// A second persist build over the same root+cache finds a compatible
    /// meta block and reuses the cache instead of a cold rebuild.
    #[test]
    fn persist_second_build_reuses_compatible_cache() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache.db");
        let first = DirSQL::builder()
            .root(dir.path())
            .tables(vec![Table::new(
                "CREATE TABLE t (x TEXT)",
                "*.txt",
                |_| vec![],
            )])
            .persist(Some(&cache))
            .build()
            .unwrap();
        drop(first);
        let second = DirSQL::builder()
            .root(dir.path())
            .tables(vec![Table::new(
                "CREATE TABLE t (x TEXT)",
                "*.txt",
                |_| vec![],
            )])
            .persist(Some(&cache))
            .build()
            .unwrap();
        assert!(second.query("SELECT * FROM t").is_ok());
    }

    #[test]
    fn prepare_persist_cold_start_reports_rebuild() {
        let dir = TempDir::new().unwrap();
        let tables = vec![Table::new("CREATE TABLE t (x TEXT)", "*.txt", |_| vec![])];
        let ctx = prepare_persist(dir.path(), &tables, &[], None).unwrap();
        assert!(ctx.cached.is_empty());
        assert!(!ctx.expected_meta.is_empty());
    }

    /// `snapshot_ns > mtime_ns`: the file is outside the racy window, so a
    /// matching stat is trusted without a hash confirmation.
    #[test]
    fn reconcile_scan_trusts_cache_outside_racy_window() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let stat = fake_stat();
        let fake = FakeFs::with_stat(abs.clone(), stat.clone());
        let mut cached = HashMap::new();
        cached.insert(
            ("a.txt".to_string(), "t".to_string()),
            CachedFile {
                rel_path: "a.txt".into(),
                table_name: "t".into(),
                stat: stat.clone(),
                content_hash: None,
                snapshot_ns: stat.mtime_ns + 1,
            },
        );
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached,
            expected_meta: HashMap::new(),
        };
        let scanned = vec![(abs, "t".to_string())];
        let (to_parse, trusted, deleted) =
            reconcile_scan(dir.path(), scanned, &ctx, &fake).unwrap();
        assert!(to_parse.is_empty());
        assert_eq!(trusted.len(), 1);
        assert!(deleted.is_empty());
    }

    /// A stat match but a *different* cached `table_name` is re-parsed
    /// rather than trusted.
    #[test]
    fn reconcile_scan_reparses_when_cached_table_differs() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let stat = fake_stat();
        let fake = FakeFs::with_stat(abs.clone(), stat.clone());
        let mut cached = HashMap::new();
        cached.insert(
            ("a.txt".to_string(), "other".to_string()),
            CachedFile {
                rel_path: "a.txt".into(),
                table_name: "other".into(),
                stat: stat.clone(),
                content_hash: None,
                snapshot_ns: stat.mtime_ns + 1,
            },
        );
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached,
            expected_meta: HashMap::new(),
        };
        let scanned = vec![(abs, "t".to_string())];
        let (to_parse, trusted, _deleted) =
            reconcile_scan(dir.path(), scanned, &ctx, &fake).unwrap();
        assert_eq!(to_parse.len(), 1);
        assert!(trusted.is_empty());
    }

    #[test]
    fn reconcile_scan_reports_deleted_cached_files() {
        let dir = TempDir::new().unwrap();
        let mut cached = HashMap::new();
        cached.insert(
            ("gone.txt".to_string(), "t".to_string()),
            CachedFile {
                rel_path: "gone.txt".into(),
                table_name: "t".into(),
                stat: fake_stat(),
                content_hash: None,
                snapshot_ns: 0,
            },
        );
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached,
            expected_meta: HashMap::new(),
        };
        let fake = FakeFs::default();
        let (to_parse, trusted, deleted) =
            reconcile_scan(dir.path(), Vec::new(), &ctx, &fake).unwrap();
        assert!(to_parse.is_empty());
        assert!(trusted.is_empty());
        assert_eq!(deleted, vec![("gone.txt".to_string(), "t".to_string())]);
    }

    #[test]
    fn compute_stat_virtuals_reads_real_metadata() {
        let dir = TempDir::new().unwrap();
        let stat = compute_stat_virtuals("d", dir.path());
        assert_eq!(stat[STAT_PATH], Value::Text("d".into()));
        assert!(
            matches!(stat.get(STAT_SIZE), Some(Value::Integer(_))),
            "size present"
        );
        assert!(
            matches!(stat.get(STAT_MTIME), Some(Value::Integer(_))),
            "mtime present"
        );
    }

    #[test]
    fn find_capture_column_collision_flags_a_placeholder_naming_a_column() {
        let declared = vec!["thread_id".to_string(), "basename".to_string()];
        assert_eq!(
            find_capture_column_collision("_comments/{thread_id}/*.txt", &declared),
            Some("thread_id".to_string())
        );
    }

    #[test]
    fn find_capture_column_collision_ignores_a_placeholder_with_no_column() {
        let declared = vec!["path".to_string(), "basename".to_string()];
        assert_eq!(
            find_capture_column_collision("_comments/{thread_id}/*.txt", &declared),
            None
        );
    }

    #[test]
    fn find_capture_column_collision_none_without_placeholders() {
        let declared = vec!["thread_id".to_string()];
        assert_eq!(
            find_capture_column_collision("_comments/*/*.txt", &declared),
            None
        );
    }

    #[test]
    fn find_capture_column_collision_returns_first_colliding_placeholder() {
        let declared = vec!["repo".to_string(), "org".to_string()];
        assert_eq!(
            find_capture_column_collision("{org}/{repo}/data.json", &declared),
            Some("org".to_string())
        );
    }

    #[test]
    fn capture_column_collision_error_names_placeholder_and_fix() {
        let err = DirSqlError::CaptureColumnCollision {
            placeholder: "thread_id".to_string(),
            column: "thread_id".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thread_id"));
        assert!(msg.contains("collides"));
        assert!(msg.contains("on-file"));
    }

    #[test]
    fn build_tables_from_config_creates_on_file_tables() {
        let cfg = config::load_config_str(concat!(
            "[[table]]\n",
            "ddl = \"CREATE TABLE a (x TEXT)\"\n",
            "glob = \"*.a\"\n",
            "on-file = \"printf '[{\\\"x\\\":1}]'\"\n\n",
            "[[table]]\n",
            "ddl = \"CREATE TABLE b (y TEXT)\"\n",
            "glob = \"*.b\"\n",
            "on-file = \"echo hi\"\n",
            "strict = true\n",
        ))
        .unwrap();
        let dir = TempDir::new().unwrap();
        let tables = build_tables_from_config(
            &cfg,
            dir.path(),
            dir.path(),
            command::DEFAULT_COMMAND_TIMEOUT,
        )
        .unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(
            (tables[0].on_file)(&dir.path().join("f.a").to_string_lossy())
                .unwrap()
                .len(),
            1
        );
        assert!(tables[1].strict, "on-file table preserves strict flag");
    }

    #[test]
    fn build_tables_from_config_uses_the_caller_supplied_timeout() {
        // The timeout is now threaded in as an explicit argument rather than
        // re-derived from `cfg.hook_timeout` inside the function; a table built
        // from a config declaring its own `hook-timeout` must honor the value
        // the caller passes, independent of the config key.
        let cfg = config::load_config_str(concat!(
            "[dirsql]\n",
            "hook-timeout = 999\n\n",
            "[[table]]\n",
            "ddl = \"CREATE TABLE b (y TEXT)\"\n",
            "glob = \"*.b\"\n",
            "on-file = \"printf '[{\\\"y\\\":1}]'\"\n",
        ))
        .unwrap();
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("f.b");
        let tables =
            build_tables_from_config(&cfg, dir.path(), dir.path(), Duration::from_secs(5)).unwrap();
        assert_eq!(tables.len(), 1);
        // The passed timeout is generous, so the on-file command runs and its
        // row is produced -- proving the argument path is live.
        assert_eq!(
            (tables[0].on_file)(&abs.to_string_lossy()).unwrap().len(),
            1
        );
    }

    #[test]
    fn run_on_file_parses_command_json_output() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("f.txt");
        // The template omits every placeholder, so nothing is appended; the
        // `printf` payload is the whole output.
        let rows = run_on_file(
            "printf '[{\"n\":1}]'",
            &abs.to_string_lossy(),
            dir.path(),
            dir.path(),
            command::DEFAULT_COMMAND_TIMEOUT,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["n"], Value::Integer(1));
    }

    /// `{abspath}` is not in the substitution table: it is left literal like any
    /// unknown `{…}`, so `printf` receives the string `{abspath}` verbatim. The
    /// template references `{path}` so that arg is the real path (and no path is
    /// appended), isolating the `{abspath}` behavior in the `q` column.
    #[test]
    fn run_on_file_does_not_substitute_abspath() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("f.txt");
        let rows = run_on_file(
            r#"printf '[{"p":"%s","q":"%s"}]' {path} {abspath}"#,
            &abs.to_string_lossy(),
            dir.path(),
            dir.path(),
            command::DEFAULT_COMMAND_TIMEOUT,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["q"], Value::Text("{abspath}".into()));
    }

    /// `{path}` interpolates the matched file's **absolute** path (not a
    /// root-relative one). The command echoes its `{path}` argument back as a
    /// row value, and we assert it is byte-for-byte the absolute path even when
    /// the file sits directly under `root` (the case the old `strip_prefix`
    /// would have shortened to a bare relative path).
    #[test]
    fn run_on_file_passes_absolute_path_for_path_placeholder() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("f.txt");
        let rows = run_on_file(
            r#"sh -c "printf '[{\"p\":\"%s\"}]' \"$1\"" sh {path}"#,
            &abs.to_string_lossy(),
            dir.path(),
            dir.path(),
            command::DEFAULT_COMMAND_TIMEOUT,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]["p"],
            Value::Text(abs.to_string_lossy().into_owned())
        );
    }

    /// A command that cannot be spawned yields no rows (per-file isolation).
    #[test]
    fn run_on_file_returns_no_rows_on_spawn_failure() {
        let dir = TempDir::new().unwrap();
        let rows = run_on_file(
            "definitely-not-a-real-binary-xyzzy",
            "/outside/f.txt",
            dir.path(),
            dir.path(),
            command::DEFAULT_COMMAND_TIMEOUT,
        );
        assert!(rows.is_empty());
    }

    #[test]
    fn run_on_file_returns_no_rows_on_non_json_output() {
        let dir = TempDir::new().unwrap();
        let rows = run_on_file(
            "echo not-json",
            "/outside/f.txt",
            dir.path(),
            dir.path(),
            command::DEFAULT_COMMAND_TIMEOUT,
        );
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn async_dirsql_builds_queries_and_forwards() {
        let dir = TempDir::new().unwrap();
        let adb = AsyncDirSQL::new(dir.path(), Vec::<Table>::new()).unwrap();
        adb.ready().await.unwrap();
        let rows = adb.query("SELECT 1 AS n").await.unwrap();
        assert_eq!(rows[0]["n"], Value::Integer(1));
        assert!(adb.sync().unwrap().query("SELECT 1").is_ok());
        adb.start_watching().unwrap();
        assert!(
            adb.poll_events(Duration::from_millis(0))
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn async_dirsql_watch_forwards() {
        let dir = TempDir::new().unwrap();
        let adb = AsyncDirSQL::new(dir.path(), Vec::<Table>::new()).unwrap();
        adb.ready().await.unwrap();
        let _stream = adb.watch().unwrap();
    }

    #[test]
    fn async_dirsql_with_ignore_constructs() {
        let dir = TempDir::new().unwrap();
        assert!(
            AsyncDirSQL::with_ignore(dir.path(), Vec::<Table>::new(), Vec::<String>::new()).is_ok()
        );
    }

    /// The async `from_config_path` shortcut fails fast (before spawning) when
    /// the config file is missing.
    #[test]
    fn async_dirsql_from_config_path_errors_on_missing_file() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("no.toml");
        assert!(AsyncDirSQL::from_config_path(&missing).is_err());
    }

    /// `sync` before init completes reports "not ready". Built directly from
    /// an empty `OnceCell` so the state is deterministic (no build race).
    #[test]
    fn async_dirsql_sync_before_ready_is_not_ready() {
        let inner = Arc::new(AsyncDirSqlInner::empty());
        let adb = AsyncDirSQL { inner };
        let err = match adb.sync() {
            Ok(_) => panic!("expected not-ready error"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("not ready"), "got: {err}");
    }

    #[tokio::test]
    async fn async_dirsql_surfaces_init_failure() {
        let inner = Arc::new(AsyncDirSqlInner::empty());
        inner
            .db
            .set(Err(DirSqlError::Ddl("boom".into())))
            .ok()
            .expect("cell was empty");
        let adb = AsyncDirSQL { inner };
        let rerr = adb.ready().await.unwrap_err();
        assert!(rerr.to_string().contains("init failed"), "got: {rerr}");
        let serr = match adb.sync() {
            Ok(_) => panic!("expected init-failed error"),
            Err(e) => e,
        };
        assert!(serr.to_string().contains("init failed"), "got: {serr}");
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
        // 10^19 exceeds i64::MAX but fits u64.
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
