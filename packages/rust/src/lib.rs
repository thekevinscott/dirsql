//! `dirsql` — an ephemeral SQL index over a local directory.
//!
//! The published crate surface is intentionally small: [`DirSQL`], [`AsyncDirSQL`],
//! [`Table`], [`Row`], [`RowEvent`], [`Value`], [`DirSqlError`]. Internal modules
//! (`config`, `db`, `differ`, `matcher`, `parser`, `scanner`, `watcher`) are
//! marked `#[doc(hidden)]`: they remain callable so in-crate benches and language
//! bindings in this workspace can reach them, but they are not part of the
//! stable public API.

#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod db;
#[doc(hidden)]
pub mod differ;
#[doc(hidden)]
pub mod matcher;
#[doc(hidden)]
pub mod parser;
#[doc(hidden)]
pub mod scanner;
#[doc(hidden)]
pub mod watcher;

use crate::db::{Db, parse_table_name};
use crate::matcher::{TableMatcher, parse_captures};
use crate::parser::ColumnSource;
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

pub use crate::db::{DbError, Value};
pub use crate::differ::RowEvent;

pub type Row = HashMap<String, Value>;
pub type WatchStream = UnboundedReceiver<RowEvent>;

type BoxError = Box<dyn StdError + Send + Sync + 'static>;
type ExtractFn =
    dyn Fn(&str, &str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static;

#[derive(Debug, Error)]
pub enum DirSqlError {
    #[error(transparent)]
    Core(#[from] DbError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("failed to lock shared state: {0}")]
    Lock(String),

    #[error("glob matcher error: {0}")]
    Matcher(String),

    #[error("watch already started")]
    WatchAlreadyStarted,

    #[error("watcher error: {0}")]
    Watch(String),

    #[error("table DDL could not be parsed: {0}")]
    Ddl(String),

    #[error("duplicate table name: {0}")]
    DuplicateTable(String),

    #[error("extract error for {path}: {message}")]
    Extract { path: String, message: String },

    #[error("config error: {0}")]
    Config(String),

    #[error("no format for table '{0}': specify format explicitly or use a recognized extension")]
    NoFormat(String),

    #[error(
        "query() only accepts read-only statements; SQLite classified this statement as a write"
    )]
    WriteForbidden,
}

pub type Result<T> = std::result::Result<T, DirSqlError>;

/// A single table definition: DDL + glob + extract callback.
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
        F: Fn(&str, &str) -> Vec<Row> + Send + Sync + 'static,
    {
        Self::try_new(ddl, glob, move |path, content| {
            Ok::<Vec<Row>, BoxError>(extract(path, content))
        })
    }

    pub fn strict<F>(ddl: impl Into<String>, glob: impl Into<String>, extract: F) -> Self
    where
        F: Fn(&str, &str) -> Vec<Row> + Send + Sync + 'static,
    {
        let mut table = Self::new(ddl, glob, extract);
        table.strict = true;
        table
    }

    pub fn try_new<F>(ddl: impl Into<String>, glob: impl Into<String>, extract: F) -> Self
    where
        F: Fn(&str, &str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static,
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
}

#[derive(Clone)]
pub struct DirSQL {
    inner: Arc<DirSqlInner>,
}

impl DirSQL {
    /// Construct a `DirSQL` over `root` using the provided tables. Blocks on
    /// the initial directory scan.
    pub fn new(root: impl Into<PathBuf>, tables: Vec<Table>) -> Result<Self> {
        Self::with_ignore(root, tables, std::iter::empty::<String>())
    }

    /// Construct a `DirSQL` from a `.dirsql.toml` located at `root/.dirsql.toml`.
    pub fn from_config(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        Self::from_config_path(root.join(".dirsql.toml"))
    }

    /// Construct a `DirSQL` from an explicit path to a `.dirsql.toml` file.
    /// The root directory is taken as the config file's parent.
    pub fn from_config_path(config_path: impl AsRef<Path>) -> Result<Self> {
        let path = config_path.as_ref();
        let cfg = config::load_config(path).map_err(|e| DirSqlError::Config(e.to_string()))?;
        let root = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let tables = build_tables_from_config(&cfg)?;
        Self::build(root, tables, cfg.ignore)
    }

    pub fn with_ignore<I, S>(
        root: impl Into<PathBuf>,
        tables: Vec<Table>,
        ignore: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::build(
            root.into(),
            tables,
            ignore.into_iter().map(Into::into).collect(),
        )
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
        let db = self
            .inner
            .db
            .lock()
            .map_err(|e| DirSqlError::Lock(e.to_string()))?;
        db.query(sql).map_err(map_db_error)
    }

    /// Lazily create the filesystem watcher. Idempotent; subsequent calls are
    /// no-ops. Called implicitly by [`poll_events`](Self::poll_events) and
    /// [`watch`](Self::watch).
    pub fn start_watching(&self) -> Result<()> {
        let mut guard = self
            .inner
            .watcher
            .lock()
            .map_err(|e| DirSqlError::Lock(e.to_string()))?;
        if guard.is_none() {
            let watcher =
                Watcher::new(&self.inner.root).map_err(|e| DirSqlError::Watch(e.to_string()))?;
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
            return Err(DirSqlError::Watch(
                "watch() is active; cannot mix with poll_events()".into(),
            ));
        }
        self.inner.poll_used.store(true, Ordering::SeqCst);
        self.start_watching()?;
        self.poll_once(timeout)
    }

    /// Channel-based watch API. Spawns a background thread that pushes
    /// [`RowEvent`]s into the returned stream. Intended for long-running Rust
    /// consumers (e.g. a CLI `watch` command). Can only be called once per
    /// `DirSQL` instance.
    ///
    /// Mutually exclusive with [`poll_events`](Self::poll_events).
    pub fn watch(&self) -> Result<WatchStream> {
        if self.inner.poll_used.load(Ordering::SeqCst) {
            return Err(DirSqlError::Watch(
                "poll_events() already in use; cannot call watch()".into(),
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
            let guard = self
                .inner
                .watcher
                .lock()
                .map_err(|e| DirSqlError::Lock(e.to_string()))?;
            let watcher = guard
                .as_ref()
                .ok_or_else(|| DirSqlError::Watch("watcher not started".into()))?;
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
        let rel_path_buf = abs_path
            .strip_prefix(&self.inner.root)
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
        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        let extract = match self.inner.extract_map.get(table) {
            Some(e) => e,
            None => return Vec::new(),
        };

        let raw_rows = match extract(rel_path, &content) {
            Ok(r) => r,
            Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
        };

        let strict = *self.inner.strict_map.get(table).unwrap_or(&false);

        let new_rows = {
            let db = match self.inner.db.lock() {
                Ok(g) => g,
                Err(e) => return vec![error_event(Some(table), rel_path, e.to_string())],
            };
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

    pub(crate) fn build(
        root: PathBuf,
        tables: Vec<Table>,
        ignore_patterns: Vec<String>,
    ) -> Result<Self> {
        let db = Db::new()?;
        let mut extract_map: HashMap<String, Arc<ExtractFn>> = HashMap::new();
        let mut strict_map: HashMap<String, bool> = HashMap::new();
        let mut mappings: Vec<(String, String)> = Vec::with_capacity(tables.len());

        for table in tables {
            let table_name =
                parse_table_name(&table.ddl).ok_or_else(|| DirSqlError::Ddl(table.ddl.clone()))?;
            if extract_map.contains_key(&table_name) {
                return Err(DirSqlError::DuplicateTable(table_name));
            }
            db.create_table(&table.ddl)?;
            mappings.push((table.glob.clone(), table_name.clone()));
            extract_map.insert(table_name.clone(), table.extract);
            strict_map.insert(table_name, table.strict);
        }

        let mapping_refs: Vec<(&str, &str)> = mappings
            .iter()
            .map(|(g, n)| (g.as_str(), n.as_str()))
            .collect();
        let ignore_refs: Vec<&str> = ignore_patterns.iter().map(String::as_str).collect();
        let matcher = TableMatcher::new(&mapping_refs, &ignore_refs)
            .map_err(|e| DirSqlError::Matcher(e.to_string()))?;

        let files = scan_directory(&root, &matcher);
        let mut file_rows: HashMap<String, (String, Vec<Row>)> = HashMap::new();

        for (file_path, table_name) in files {
            let content = std::fs::read_to_string(&file_path)?;
            let rel_path = relative_path(&root, &file_path);
            let extract = extract_map.get(&table_name).ok_or_else(|| {
                DirSqlError::Ddl(format!("missing extract function for table {table_name}"))
            })?;
            let strict = *strict_map.get(&table_name).unwrap_or(&false);
            let raw_rows = extract(&rel_path, &content).map_err(|e| DirSqlError::Extract {
                path: rel_path.clone(),
                message: e.to_string(),
            })?;

            let mut rows = Vec::with_capacity(raw_rows.len());
            for (row_index, raw_row) in raw_rows.iter().enumerate() {
                let row = db.normalize_row(&table_name, raw_row, strict)?;
                db.insert_row(&table_name, &row, &rel_path, row_index)?;
                rows.push(row);
            }

            file_rows.insert(rel_path, (table_name, rows));
        }

        Ok(Self {
            inner: Arc::new(DirSqlInner {
                db: Mutex::new(db),
                root,
                matcher,
                extract_map,
                strict_map,
                file_rows: Mutex::new(file_rows),
                watcher: Mutex::new(None),
                poll_used: AtomicBool::new(false),
                watch_thread_started: AtomicBool::new(false),
            }),
        })
    }
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
    loop {
        match db.poll_once(Duration::from_millis(200)) {
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

/// Build [`Table`] objects from a parsed config by synthesizing an extract
/// closure from the declared `format` / `each` / `columns`.
fn build_tables_from_config(cfg: &config::Config) -> Result<Vec<Table>> {
    let mut tables = Vec::with_capacity(cfg.tables.len());

    for table_cfg in &cfg.tables {
        let format = table_cfg.format.ok_or_else(|| {
            let name = parse_table_name(&table_cfg.ddl).unwrap_or_else(|| table_cfg.glob.clone());
            DirSqlError::NoFormat(name)
        })?;

        let each = table_cfg.each.clone();
        let glob = table_cfg.glob.clone();
        let (_, capture_names, capture_regex) = parse_captures(&glob);

        let column_sources: HashMap<String, ColumnSource> = table_cfg
            .columns
            .as_ref()
            .map(|cols| {
                cols.iter()
                    .map(|(col_name, source_str)| {
                        (
                            col_name.clone(),
                            ColumnSource::parse(source_str, &capture_names),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut table = Table::try_new(
            table_cfg.ddl.clone(),
            table_cfg.glob.clone(),
            move |path: &str, content: &str| {
                let mut rows = parser::parse_file(format, content, each.as_deref())
                    .map_err(|e| -> BoxError { Box::new(e) })?;

                let captures: HashMap<String, String> = if let Some(ref regex) = capture_regex {
                    regex
                        .captures(path)
                        .map(|caps| {
                            capture_names
                                .iter()
                                .filter_map(|name| {
                                    caps.name(name)
                                        .map(|m| (name.clone(), m.as_str().to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    HashMap::new()
                };

                rows = parser::apply_columns(&rows, &column_sources, &captures);
                Ok(rows)
            },
        );

        if table_cfg.strict == Some(true) {
            table.strict = true;
        }

        tables.push(table);
    }

    Ok(tables)
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
    pub fn new(root: impl Into<PathBuf>, tables: Vec<Table>) -> Result<Self> {
        Self::with_ignore(root, tables, std::iter::empty::<String>())
    }

    pub fn from_config(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        Self::from_config_path(root.join(".dirsql.toml"))
    }

    pub fn from_config_path(config_path: impl AsRef<Path>) -> Result<Self> {
        let path = config_path.as_ref();
        let cfg = config::load_config(path).map_err(|e| DirSqlError::Config(e.to_string()))?;
        let root = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let tables = build_tables_from_config(&cfg)?;
        Ok(Self::spawn_build(root, tables, cfg.ignore))
    }

    pub fn with_ignore<I, S>(
        root: impl Into<PathBuf>,
        tables: Vec<Table>,
        ignore: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let root = root.into();
        let ignore_patterns: Vec<String> = ignore.into_iter().map(Into::into).collect();
        Ok(Self::spawn_build(root, tables, ignore_patterns))
    }

    fn spawn_build(root: PathBuf, tables: Vec<Table>, ignore: Vec<String>) -> Self {
        let inner = Arc::new(AsyncDirSqlInner {
            db: tokio::sync::OnceCell::new(),
            ready_notify: tokio::sync::Notify::new(),
        });
        let inner_clone = inner.clone();
        thread::spawn(move || {
            let result = DirSQL::build(root, tables, ignore);
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
        assert!(matches!(err, DirSqlError::WriteForbidden));
    }

    #[test]
    fn map_db_error_leaves_sqlite_errors_as_core() {
        let err = map_db_error(DbError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("syntax error".into()),
        )));
        assert!(matches!(err, DirSqlError::Core(_)));
    }

    #[test]
    fn map_db_error_leaves_schema_mismatch_as_core() {
        let err = map_db_error(DbError::SchemaMismatch("nope".into()));
        assert!(matches!(err, DirSqlError::Core(_)));
    }
}
