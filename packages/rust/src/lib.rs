use dirsql_core::config;
use dirsql_core::db::{Db, parse_table_name};
use dirsql_core::differ::{self, RowEvent as CoreRowEvent};
use dirsql_core::matcher::{TableMatcher, parse_captures};
use dirsql_core::parser::{self, ColumnSource};
use dirsql_core::scanner::scan_directory;
use dirsql_core::watcher::{FileEvent, Watcher};
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

pub type Row = HashMap<String, Value>;
pub type WatchStream = UnboundedReceiver<RowEvent>;

type BoxError = Box<dyn StdError + Send + Sync + 'static>;
type ExtractFn =
    dyn Fn(&str, &str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static;

pub use dirsql_core::db::Value;
pub use dirsql_core::differ::RowEvent;

#[derive(Debug, Error)]
pub enum DirSqlError {
    #[error(transparent)]
    Core(#[from] dirsql_core::db::DbError),

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
}

pub type Result<T> = std::result::Result<T, DirSqlError>;

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
        let ddl = ddl.into();
        let glob = glob.into();
        let extract = Arc::new(move |path: &str, content: &str| extract(path, content));

        Self {
            ddl,
            glob,
            extract,
            strict: false,
        }
    }
}

struct TableConfig {
    name: String,
    glob: String,
    extract: Arc<ExtractFn>,
    strict: bool,
}

type FileRows = (String, Vec<Row>);

struct DirSqlInner {
    db: Arc<Mutex<Db>>,
    root: PathBuf,
    table_configs: Vec<TableConfig>,
    ignore_patterns: Vec<String>,
    file_rows: Arc<Mutex<HashMap<String, FileRows>>>,
    watch_started: AtomicBool,
}

#[derive(Clone)]
pub struct DirSQL {
    inner: Arc<DirSqlInner>,
}

impl DirSQL {
    pub fn new(root: impl Into<PathBuf>, tables: Vec<Table>) -> Result<Self> {
        Self::with_ignore(root, tables, std::iter::empty::<String>())
    }

    /// Create a `DirSQL` instance from a `.dirsql.toml` config file.
    ///
    /// Looks for `.dirsql.toml` in the given root directory, parses it,
    /// and generates extract functions from the declared format/each/columns.
    pub fn from_config(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let config_path = root.join(".dirsql.toml");
        let cfg =
            config::load_config(&config_path).map_err(|e| DirSqlError::Config(e.to_string()))?;

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

    pub fn query(&self, sql: &str) -> Result<Vec<Row>> {
        let db = self
            .inner
            .db
            .lock()
            .map_err(|err| DirSqlError::Lock(err.to_string()))?;
        db.query(sql).map_err(Into::into)
    }

    pub fn watch(&self) -> Result<WatchStream> {
        if self.inner.watch_started.swap(true, Ordering::SeqCst) {
            return Err(DirSqlError::WatchAlreadyStarted);
        }

        let watcher =
            Watcher::new(&self.inner.root).map_err(|err| DirSqlError::Watch(err.to_string()))?;
        let (tx, rx) = unbounded();
        let inner = self.inner.clone();

        thread::spawn(move || run_watch_loop(inner, watcher, tx));

        Ok(rx)
    }

    pub(crate) fn build(
        root: PathBuf,
        tables: Vec<Table>,
        ignore_patterns: Vec<String>,
    ) -> Result<Self> {
        let db = Arc::new(Mutex::new(Db::new()?));
        let file_rows = Arc::new(Mutex::new(HashMap::new()));
        let mut table_configs = Vec::new();
        let mut seen_table_names = HashMap::new();

        {
            let db_guard = db
                .lock()
                .map_err(|err| DirSqlError::Lock(err.to_string()))?;
            for table in tables {
                let table_name = parse_table_name(&table.ddl)
                    .ok_or_else(|| DirSqlError::Ddl(table.ddl.clone()))?;
                if seen_table_names.insert(table_name.clone(), ()).is_some() {
                    return Err(DirSqlError::DuplicateTable(table_name));
                }
                db_guard.create_table(&table.ddl)?;
                table_configs.push(TableConfig {
                    name: table_name,
                    glob: table.glob,
                    extract: table.extract,
                    strict: table.strict,
                });
            }
        }

        let mappings: Vec<(&str, &str)> = table_configs
            .iter()
            .map(|cfg| (cfg.glob.as_str(), cfg.name.as_str()))
            .collect();
        let ignore_refs: Vec<&str> = ignore_patterns.iter().map(String::as_str).collect();
        let matcher = TableMatcher::new(&mappings, &ignore_refs)
            .map_err(|err| DirSqlError::Matcher(err.to_string()))?;
        let extract_map: HashMap<String, Arc<ExtractFn>> = table_configs
            .iter()
            .map(|cfg| (cfg.name.clone(), cfg.extract.clone()))
            .collect();
        let strict_map: HashMap<String, bool> = table_configs
            .iter()
            .map(|cfg| (cfg.name.clone(), cfg.strict))
            .collect();
        let files = scan_directory(&root, &matcher);

        {
            let db_guard = db
                .lock()
                .map_err(|err| DirSqlError::Lock(err.to_string()))?;
            let mut file_rows_guard = file_rows
                .lock()
                .map_err(|err| DirSqlError::Lock(err.to_string()))?;

            for (file_path, table_name) in files {
                let content = std::fs::read_to_string(&file_path)?;
                let rel_path = relative_path(&root, &file_path);
                let extract = extract_map.get(&table_name).ok_or_else(|| {
                    DirSqlError::Ddl(format!("missing extract function for table {table_name}"))
                })?;
                let strict = *strict_map.get(&table_name).unwrap_or(&false);
                let raw_rows =
                    extract(&rel_path, &content).map_err(|err| DirSqlError::Extract {
                        path: rel_path.clone(),
                        message: err.to_string(),
                    })?;

                let mut rows = Vec::with_capacity(raw_rows.len());
                for (row_index, raw_row) in raw_rows.iter().enumerate() {
                    let row = db_guard.normalize_row(&table_name, raw_row, strict)?;
                    db_guard.insert_row(&table_name, &row, &rel_path, row_index)?;
                    rows.push(row);
                }

                file_rows_guard.insert(rel_path, (table_name, rows));
            }
        }

        Ok(Self {
            inner: Arc::new(DirSqlInner {
                db,
                root,
                table_configs,
                ignore_patterns,
                file_rows,
                watch_started: AtomicBool::new(false),
            }),
        })
    }
}

fn run_watch_loop(inner: Arc<DirSqlInner>, watcher: Watcher, tx: UnboundedSender<RowEvent>) {
    let mappings: Vec<(&str, &str)> = inner
        .table_configs
        .iter()
        .map(|cfg| (cfg.glob.as_str(), cfg.name.as_str()))
        .collect();
    let ignore_refs: Vec<&str> = inner.ignore_patterns.iter().map(String::as_str).collect();
    let matcher = match TableMatcher::new(&mappings, &ignore_refs) {
        Ok(matcher) => matcher,
        Err(err) => {
            let _ = tx.unbounded_send(CoreRowEvent::Error {
                file_path: inner.root.clone(),
                error: err.to_string(),
            });
            return;
        }
    };
    let extract_map: HashMap<String, Arc<ExtractFn>> = inner
        .table_configs
        .iter()
        .map(|cfg| (cfg.name.clone(), cfg.extract.clone()))
        .collect();
    let strict_map: HashMap<String, bool> = inner
        .table_configs
        .iter()
        .map(|cfg| (cfg.name.clone(), cfg.strict))
        .collect();

    loop {
        let mut file_events = Vec::new();
        match watcher.recv_timeout(Duration::from_millis(200)) {
            Some(event) => {
                file_events.push(event);
                file_events.extend(watcher.try_recv_all());
            }
            None => continue,
        }

        for file_event in file_events {
            let abs_path = match &file_event {
                FileEvent::Created(path) | FileEvent::Modified(path) | FileEvent::Deleted(path) => {
                    path
                }
            };

            let rel = abs_path.strip_prefix(&inner.root).unwrap_or(abs_path);

            if matcher.is_ignored(rel) {
                continue;
            }

            let table_name = match matcher.match_file(rel) {
                Some(name) => name.to_string(),
                None => continue,
            };
            let rel_path = relative_path(&inner.root, abs_path);

            match file_event {
                FileEvent::Deleted(_) => {
                    let old_rows = {
                        let mut file_rows = match inner.file_rows.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                let _ = tx.unbounded_send(CoreRowEvent::Error {
                                    file_path: PathBuf::from(&rel_path),
                                    error: err.to_string(),
                                });
                                continue;
                            }
                        };
                        file_rows.remove(&rel_path).map(|(_, rows)| rows)
                    };
                    let row_events =
                        differ::diff(&table_name, old_rows.as_deref(), None, &rel_path);

                    let delete_result = {
                        let db = match inner.db.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                let _ = tx.unbounded_send(CoreRowEvent::Error {
                                    file_path: PathBuf::from(&rel_path),
                                    error: err.to_string(),
                                });
                                continue;
                            }
                        };
                        db.delete_rows_by_file(&table_name, &rel_path)
                    };

                    if let Err(err) = delete_result {
                        let _ = tx.unbounded_send(CoreRowEvent::Error {
                            file_path: PathBuf::from(&rel_path),
                            error: err.to_string(),
                        });
                        continue;
                    }

                    for event in row_events {
                        if tx.unbounded_send(event).is_err() {
                            return;
                        }
                    }
                }
                FileEvent::Created(_) | FileEvent::Modified(_) => {
                    let content = match std::fs::read_to_string(abs_path) {
                        Ok(content) => content,
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(err) => {
                            let _ = tx.unbounded_send(CoreRowEvent::Error {
                                file_path: PathBuf::from(&rel_path),
                                error: err.to_string(),
                            });
                            continue;
                        }
                    };

                    let extract = match extract_map.get(&table_name) {
                        Some(extract) => extract,
                        None => continue,
                    };

                    let raw_rows = match extract(&rel_path, &content) {
                        Ok(rows) => rows,
                        Err(err) => {
                            let _ = tx.unbounded_send(CoreRowEvent::Error {
                                file_path: PathBuf::from(&rel_path),
                                error: err.to_string(),
                            });
                            continue;
                        }
                    };

                    // Normalize rows according to schema
                    let strict = *strict_map.get(&table_name).unwrap_or(&false);
                    let new_rows = {
                        let db = match inner.db.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                let _ = tx.unbounded_send(CoreRowEvent::Error {
                                    file_path: PathBuf::from(&rel_path),
                                    error: err.to_string(),
                                });
                                continue;
                            }
                        };
                        let mut normalized = Vec::with_capacity(raw_rows.len());
                        let mut had_error = false;
                        for raw_row in &raw_rows {
                            match db.normalize_row(&table_name, raw_row, strict) {
                                Ok(row) => normalized.push(row),
                                Err(err) => {
                                    let _ = tx.unbounded_send(CoreRowEvent::Error {
                                        file_path: PathBuf::from(&rel_path),
                                        error: err.to_string(),
                                    });
                                    had_error = true;
                                    break;
                                }
                            }
                        }
                        if had_error {
                            continue;
                        }
                        normalized
                    };

                    let old_rows = {
                        let file_rows = match inner.file_rows.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                let _ = tx.unbounded_send(CoreRowEvent::Error {
                                    file_path: PathBuf::from(&rel_path),
                                    error: err.to_string(),
                                });
                                continue;
                            }
                        };
                        file_rows.get(&rel_path).map(|(_, rows)| rows.clone())
                    };

                    let row_events =
                        differ::diff(&table_name, old_rows.as_deref(), Some(&new_rows), &rel_path);

                    let db_result = {
                        let db = match inner.db.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                let _ = tx.unbounded_send(CoreRowEvent::Error {
                                    file_path: PathBuf::from(&rel_path),
                                    error: err.to_string(),
                                });
                                continue;
                            }
                        };
                        db.delete_rows_by_file(&table_name, &rel_path)
                            .and_then(|_| {
                                for (row_index, row) in new_rows.iter().enumerate() {
                                    db.insert_row(&table_name, row, &rel_path, row_index)?;
                                }
                                Ok(())
                            })
                    };

                    if let Err(err) = db_result {
                        let _ = tx.unbounded_send(CoreRowEvent::Error {
                            file_path: PathBuf::from(&rel_path),
                            error: err.to_string(),
                        });
                        continue;
                    }

                    {
                        let mut file_rows = match inner.file_rows.lock() {
                            Ok(guard) => guard,
                            Err(err) => {
                                let _ = tx.unbounded_send(CoreRowEvent::Error {
                                    file_path: PathBuf::from(&rel_path),
                                    error: err.to_string(),
                                });
                                continue;
                            }
                        };
                        file_rows.insert(rel_path.clone(), (table_name.clone(), new_rows));
                    }

                    for event in row_events {
                        if tx.unbounded_send(event).is_err() {
                            return;
                        }
                    }
                }
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

/// Build `Table` objects from a parsed config.
///
/// For each table entry, generates an extract closure that uses the core
/// parser to parse the file content, applies path captures from the glob
/// pattern, and maps columns as specified.
fn build_tables_from_config(cfg: &config::Config) -> Result<Vec<Table>> {
    let mut tables = Vec::with_capacity(cfg.tables.len());

    for table_cfg in &cfg.tables {
        let format = table_cfg.format.ok_or_else(|| {
            // Extract table name from DDL for the error message
            let name = parse_table_name(&table_cfg.ddl).unwrap_or_else(|| table_cfg.glob.clone());
            DirSqlError::NoFormat(name)
        })?;

        let each = table_cfg.each.clone();
        let glob = table_cfg.glob.clone();

        // Parse capture info from the glob pattern
        let (_, capture_names, capture_regex) = parse_captures(&glob);

        // Build column sources if columns are specified
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

        let table = Table::try_new(
            table_cfg.ddl.clone(),
            table_cfg.glob.clone(),
            move |path: &str, content: &str| {
                // 1. Parse file content
                let mut rows = parser::parse_file(format, content, each.as_deref())
                    .map_err(|e| -> BoxError { Box::new(e) })?;

                // 2. Extract captures from the path
                let captures: HashMap<String, String> = if let Some(ref regex) = capture_regex {
                    if let Some(caps) = regex.captures(path) {
                        capture_names
                            .iter()
                            .filter_map(|name| {
                                caps.name(name)
                                    .map(|m| (name.clone(), m.as_str().to_string()))
                            })
                            .collect()
                    } else {
                        HashMap::new()
                    }
                } else {
                    HashMap::new()
                };

                // 3. Apply column mapping and captures
                rows = parser::apply_columns(&rows, &column_sources, &captures);

                Ok(rows)
            },
        );

        let mut table = table;
        if table_cfg.strict == Some(true) {
            table.strict = true;
        }

        tables.push(table);
    }

    Ok(tables)
}

/// Async wrapper around [`DirSQL`].
///
/// Construction is non-blocking: the initial directory scan runs on a
/// background thread.  Call [`ready()`](AsyncDirSQL::ready) to wait until
/// the scan completes before issuing queries.
///
/// ```ignore
/// let db = AsyncDirSQL::new(root, vec![table])?;
/// db.ready().await?;
/// let rows = db.query("SELECT * FROM t").await?;
/// ```
#[derive(Clone)]
pub struct AsyncDirSQL {
    inner: Arc<AsyncDirSqlInner>,
}

struct AsyncDirSqlInner {
    /// Populated once the background init completes.
    db: tokio::sync::OnceCell<std::result::Result<DirSQL, DirSqlError>>,
    /// Signal that init is done (for `ready()`).
    ready_notify: tokio::sync::Notify,
}

impl AsyncDirSQL {
    /// Create a new `AsyncDirSQL`.  The constructor returns immediately;
    /// the scan runs on a background thread.
    pub fn new(root: impl Into<PathBuf>, tables: Vec<Table>) -> Result<Self> {
        Self::with_ignore(root, tables, std::iter::empty::<String>())
    }

    /// Create an `AsyncDirSQL` instance from a `.dirsql.toml` config file.
    ///
    /// Returns immediately; the initial scan runs on a background thread.
    /// Call `ready().await` before issuing queries.
    pub fn from_config(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let config_path = root.join(".dirsql.toml");
        let cfg =
            config::load_config(&config_path).map_err(|e| DirSqlError::Config(e.to_string()))?;

        let tables = build_tables_from_config(&cfg)?;
        let ignore_patterns: Vec<String> = cfg.ignore;

        let inner = Arc::new(AsyncDirSqlInner {
            db: tokio::sync::OnceCell::new(),
            ready_notify: tokio::sync::Notify::new(),
        });

        let inner_clone = inner.clone();
        thread::spawn(move || {
            let result = DirSQL::build(root, tables, ignore_patterns);
            let _ = inner_clone.db.set(result);
            inner_clone.ready_notify.notify_waiters();
        });

        Ok(Self { inner })
    }

    /// Like [`new`](Self::new) but with ignore patterns.
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

        let inner = Arc::new(AsyncDirSqlInner {
            db: tokio::sync::OnceCell::new(),
            ready_notify: tokio::sync::Notify::new(),
        });

        let inner_clone = inner.clone();
        thread::spawn(move || {
            let result = DirSQL::build(root, tables, ignore_patterns);
            // Ignore the error from set -- it can only fail if already set,
            // which shouldn't happen.
            let _ = inner_clone.db.set(result);
            inner_clone.ready_notify.notify_waiters();
        });

        Ok(Self { inner })
    }

    /// Wait until the initial scan is complete.
    ///
    /// Returns `Ok(())` on success, or propagates any error that occurred
    /// during init.  Safe to call multiple times.
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

    /// Execute a SQL query asynchronously.
    ///
    /// The query runs on a blocking thread to avoid stalling the async
    /// runtime.
    pub async fn query(&self, sql: &str) -> Result<Vec<Row>> {
        let db = self.get_db()?;
        let sql = sql.to_string();
        tokio::task::spawn_blocking(move || db.query(&sql))
            .await
            .map_err(|e| DirSqlError::Lock(format!("join error: {e}")))?
    }

    /// Start watching for file changes.  Returns a [`WatchStream`] that
    /// yields [`RowEvent`] values.
    pub fn watch(&self) -> Result<WatchStream> {
        let db = self.get_db()?;
        db.watch()
    }

    fn get_db(&self) -> Result<DirSQL> {
        match self.inner.db.get() {
            Some(Ok(db)) => Ok(db.clone()),
            Some(Err(e)) => Err(DirSqlError::Lock(format!("init failed: {e}"))),
            None => Err(DirSqlError::Lock(
                "not ready: call ready().await first".into(),
            )),
        }
    }
}
