//! `dirsql` — an ephemeral SQL index over a local directory.
//!
//! The published crate surface is intentionally small: [`DirSQL`], [`AsyncDirSQL`],
//! [`Table`], [`Row`], [`RowEvent`], [`Value`], [`DirSqlError`]. Internal modules
//! (`config`, `db`, `differ`, `matcher`, `parser`, `scanner`, `watcher`) are
//! marked `#[doc(hidden)]`: they remain callable so in-crate benches and language
//! bindings in this workspace can reach them, but they are not part of the
//! stable public API.

mod brace;
mod collating_symbol;
/// Reusable command runner backing the command-backed events.
pub mod command;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod db;
#[doc(hidden)]
pub mod differ;
#[doc(hidden)]
pub mod extension_resolution;
#[doc(hidden)]
pub mod foreign_key;
#[doc(hidden)]
pub mod functions;
mod git_excludes_file;
mod gitignore_frame;
mod glob_unescape;
#[doc(hidden)]
pub mod infer;
#[cfg(target_os = "linux")]
mod inotify_watch;
#[doc(hidden)]
pub mod launcher;
mod listing;
mod long_path;
#[doc(hidden)]
pub mod matcher;
#[cfg(not(target_os = "linux"))]
mod notify_watch;
mod on_file;
mod params;
#[doc(hidden)]
#[doc(hidden)]
pub mod parsed_vtab;
#[doc(hidden)]
pub mod path_table;
mod per_thread;
#[doc(hidden)]
pub mod persist;
mod posix_class;
#[doc(hidden)]
pub mod progress;
#[doc(hidden)]
pub mod row_event_flat;
mod row_stream;
#[doc(hidden)]
pub mod scanner;
#[doc(hidden)]
pub mod sql_literal;
mod table_refs;
mod tree_walk;
#[doc(hidden)]
pub mod vtab;
mod vtab_scaffold;
#[doc(hidden)]
pub mod watcher;

#[cfg(feature = "cli")]
pub mod cli;

use crate::config::Source;
use crate::db::{Db, ShapedRows};
use crate::functions::ResolvedFunction;
use crate::matcher::TableMatcher;
use crate::persist::{
    CachedFile, FileStat, build_meta, canonical_root, compute_glob_config_hash,
    create_sidecar_tables, delete_file as cache_delete_file, drop_user_tables, ensure_parent_dir,
    hash_file, is_trusted, meta_is_compatible, now_ns, read_cached_files, read_meta,
    resolve_persist_path, upsert_file, write_meta,
};
use crate::progress::Progress;
use crate::scanner::{Found, scan_directory_reporting};
use crate::watcher::{FileEvent, WatchFault, WatchScope, Watcher};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error as StdError;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use thiserror::Error;

pub use crate::config::ExtensionSpec as Extension;
pub use crate::db::{ColumnInfo, DbError, QueryResult, TableInfo, Value};
pub use crate::differ::RowEvent;
pub use crate::params::Params;
#[doc(hidden)]
pub use crate::row_event_flat::{FlatRowEvent, flatten_row_event};
#[doc(hidden)]
pub use crate::watcher::FileEvent as RawFileEvent;

pub type Row = HashMap<String, Value>;
pub type WatchStream = UnboundedReceiver<RowEvent>;

/// The escalation scaffold `dirsql init` writes verbatim: one named
/// `[[table]]` (glob + DDL + a real `on-file` hook) demonstrating how to pull
/// structured rows out of files, rather than duplicating the zero-config
/// path-table floor (`SELECT * FROM './'`). Carrying a genuine hook keeps it a
/// valid config even once hook-less `[[table]]` entries become a load error.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("default_config.toml");

type BoxError = Box<dyn StdError + Send + Sync + 'static>;
type OnFileFn = dyn Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static;
/// Where a per-table hook hands its rows, one chunk at a time.
type RowSink<'a> = dyn FnMut(Vec<infer::JsonRow>) + Send + 'a;
/// The paths a per-table hook runs over, yielded as the scan finds them.
type PathFeed<'a> = dyn Iterator<Item = PathBuf> + Send + 'a;
/// What a per-table hook may call once its last chunk is out, before it knows
/// whether it succeeded, so its rows can be staged while it finishes.
type OutputEnd<'a> = dyn FnMut() + 'a;
type OnFilesFn = dyn Fn(&mut PathFeed<'_>, &mut RowSink<'_>, &mut OutputEnd<'_>) -> std::result::Result<(), BoxError>
    + Send
    + Sync
    + 'static;

/// How a table turns matched files into rows: a programmatic callback runs
/// once per file; a configured `on-file` command runs once per table over
/// every matched path.
#[derive(Clone)]
enum Hook {
    PerFile(Arc<OnFileFn>),
    PerTable(Arc<OnFilesFn>),
}

/// The `file_path` a per-table hook's rows are recorded under. No real
/// root-relative path is empty, so it never collides with a per-file owner
/// and never sits beneath any directory a watcher reports deleted.
const BATCH_OWNER: &str = "";

/// One file whose on-file hook failed, carried so a scan can report every
/// failure rather than only whichever came first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnFileFailure {
    /// Path relative to the scan root.
    pub path: String,
    /// The hook's error, as rendered by its `Display`.
    pub message: String,
}

/// An error the OS reported, which each binding raises as its own native OS
/// error. Carried inside [`DirSqlError::Io`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct OsError {
    pub errno: i32,
    /// The errno's symbolic name, as Node's `code`.
    pub code: &'static str,
    pub message: &'static str,
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

    #[error(
        "table '{name}': its `ddl` ran but created no table called '{name}' \
         (it created: {}). Set `name` to the table the `ddl` creates.",
        if .created.is_empty() { "nothing".to_string() } else { .created.join(", ") }
    )]
    TableNotCreated { name: String, created: Vec<String> },

    #[error(
        "table '{name}': its `ddl` created a virtual table called '{name}'. \
         The declared table holds the file rows, so it must be a real row \
         table; create the virtual table alongside it, under its own name."
    )]
    TableIsVirtual { name: String },

    #[error("table '{name}': {source}")]
    TableDdl {
        name: String,
        #[source]
        source: DbError,
    },

    #[error("failed to load extension '{}': {source}", .path.display())]
    Extension {
        path: PathBuf,
        #[source]
        source: DbError,
    },

    #[error("duplicate table name: {0}")]
    DuplicateTable(String),

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

    #[error("{0}")]
    PathPlaceholder(String),

    #[error("table `{name}`: {message}")]
    TableCommand { name: String, message: String },

    #[error("table `{table}`, file `{path}`: {message}")]
    OnFile {
        table: String,
        path: String,
        message: String,
    },
}

impl DirSqlError {
    /// The OS error this is, when a binding should raise its native one.
    pub fn os_error(&self) -> Option<&OsError> {
        match self {
            Self::Io(e) => e.get_ref()?.downcast_ref(),
            _ => None,
        }
    }

    fn lock(e: impl std::fmt::Display) -> Self {
        DirSqlError::Lock(e.to_string())
    }

    fn watch(e: notify::Error) -> Self {
        DirSqlError::Watch {
            message: watcher::describe_watch_error(&e, cfg!(target_os = "linux")),
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
///
/// `name` is the table's SQL name, declared rather than derived from `ddl`.
/// The `ddl` must create a table by that name; the mismatch is caught against
/// SQLite's catalog at load time.
#[derive(Clone)]
pub struct Table {
    pub name: String,
    pub ddl: String,
    pub glob: String,
    pub strict: bool,
    hook: Hook,
    anchor: Option<PathBuf>,
    ignore: Vec<String>,
    warn_when_empty: bool,
}

impl Table {
    pub(crate) fn anchor_key(&self) -> Option<String> {
        self.anchor
            .as_ref()
            .map(|anchor| anchor.to_string_lossy().into_owned())
    }

    pub(crate) fn ignore_key(&self) -> &[String] {
        &self.ignore
    }

    pub(crate) fn warning_when_empty(mut self) -> Self {
        self.warn_when_empty = true;
        self
    }

    pub(crate) fn scoped_ignore(mut self, ignore: Vec<String>) -> Self {
        self.ignore = ignore;
        self
    }

    #[doc(hidden)]
    pub fn anchored(mut self, anchor: impl Into<PathBuf>) -> Self {
        self.anchor = Some(anchor.into());
        self
    }

    pub fn new<F>(
        name: impl Into<String>,
        ddl: impl Into<String>,
        glob: impl Into<String>,
        on_file: F,
    ) -> Self
    where
        F: Fn(&str) -> Vec<Row> + Send + Sync + 'static,
    {
        Self::try_new(name, ddl, glob, move |path| {
            Ok::<Vec<Row>, BoxError>(on_file(path))
        })
    }

    pub fn strict<F>(
        name: impl Into<String>,
        ddl: impl Into<String>,
        glob: impl Into<String>,
        on_file: F,
    ) -> Self
    where
        F: Fn(&str) -> Vec<Row> + Send + Sync + 'static,
    {
        let mut table = Self::new(name, ddl, glob, on_file);
        table.strict = true;
        table
    }

    pub fn try_new<F>(
        name: impl Into<String>,
        ddl: impl Into<String>,
        glob: impl Into<String>,
        on_file: F,
    ) -> Self
    where
        F: Fn(&str) -> std::result::Result<Vec<Row>, BoxError> + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            ddl: ddl.into(),
            glob: glob.into(),
            hook: Hook::PerFile(Arc::new(on_file)),
            strict: false,
            anchor: None,
            ignore: Vec::new(),
            warn_when_empty: false,
        }
    }

    /// A table whose hook runs once over every matched absolute path and
    /// returns the whole table's rows.
    #[cfg(test)]
    fn per_table<F>(
        name: impl Into<String>,
        ddl: impl Into<String>,
        glob: impl Into<String>,
        on_files: F,
    ) -> Self
    where
        F: Fn(&[PathBuf]) -> std::result::Result<Vec<infer::JsonRow>, BoxError>
            + Send
            + Sync
            + 'static,
    {
        Self::per_table_streaming(
            name,
            ddl,
            glob,
            move |feed, sink: &mut RowSink<'_>, _: &mut OutputEnd<'_>| {
                let paths: Vec<PathBuf> = feed.collect();
                sink(on_files(&paths)?);
                Ok(())
            },
        )
    }

    /// [`per_table`](Self::per_table) for a hook that hands its rows over in
    /// chunks as it produces them, so they can be stored while it runs on.
    fn per_table_streaming<F>(
        name: impl Into<String>,
        ddl: impl Into<String>,
        glob: impl Into<String>,
        on_files: F,
    ) -> Self
    where
        F: Fn(
                &mut PathFeed<'_>,
                &mut RowSink<'_>,
                &mut OutputEnd<'_>,
            ) -> std::result::Result<(), BoxError>
            + Send
            + Sync
            + 'static,
    {
        Self {
            name: name.into(),
            ddl: ddl.into(),
            glob: glob.into(),
            hook: Hook::PerTable(Arc::new(on_files)),
            strict: false,
            anchor: None,
            ignore: Vec::new(),
            warn_when_empty: false,
        }
    }
}

/// Where the applier hands each applied batch's row events.
enum Sink {
    Stream(UnboundedSender<RowEvent>),
    Poll(std::sync::mpsc::Sender<Vec<RowEvent>>),
}

struct DirSqlInner {
    db: Mutex<Db>,
    root: PathBuf,
    /// One entry per distinct table anchor; each holds the matcher for the
    /// tables anchored there.
    groups: Vec<AnchorGroup>,
    on_file_map: HashMap<String, Hook>,
    strict_map: HashMap<String, bool>,
    /// Every table's name in declaration order: per-table hooks run in it.
    declared: Vec<String>,
    /// The root-relative paths each per-table hook was last run over. Rows
    /// are recorded under [`BATCH_OWNER`], not their files, so this is how a
    /// deleted directory is traced back to the tables it fed.
    batch_files: Mutex<HashMap<String, Vec<String>>>,
    /// The file events of watch batches a foreign key rejected, replayed
    /// with every later batch until one lands them.
    held: Mutex<Vec<FileEvent>>,
    fault: OnceLock<WatchFault>,
    /// Files the initial scan could not index. Empty for a clean scan.
    scan_failures: Vec<OnFileFailure>,
    watcher: Mutex<Option<Arc<Watcher>>>,
    /// Locks out [`DirSQL::watch`] once [`DirSQL::poll_events`] has run:
    /// both would drain the same underlying watcher.
    poll_used: AtomicBool,
    /// Locks out [`DirSQL::poll_events`] once [`DirSQL::watch`] has spawned
    /// its background thread.
    watch_thread_started: AtomicBool,
    /// Where the row events of each applied batch go: to [`DirSQL::watch`]'s
    /// stream or to [`DirSQL::poll_events`], whichever attached.
    stream: Mutex<Option<Sink>>,
    /// The batches delivered for [`DirSQL::poll_events`] to return.
    polled: Mutex<Option<std::sync::mpsc::Receiver<Vec<RowEvent>>>>,
    poll_interval: Duration,
    /// Filesystem seam: [`RealFs`] in production; unit tests inject a double.
    fs: Arc<dyn FileSystem>,
    /// Held across every change to the index and every query, so a query
    /// never reads a batch half applied.
    fresh: Mutex<FreshState>,
    /// Signalled when [`DirSQL::unlock_index`] releases the index.
    unlocked: Condvar,
    /// The symlinked directories the index reaches files through. The watcher
    /// reports a change only at the link's target, so each is restated here.
    links: Mutex<Vec<Link>>,
    /// The error of each file, by table and path, whose hook or rows failed
    /// after the build. Every query raises the first while any stands.
    failed_files: Mutex<BTreeMap<(String, String), String>>,
}

/// A symlinked directory at `path`, under a watch anchor, resolving to
/// `target`.
#[derive(Debug, Clone, PartialEq)]
struct Link {
    path: PathBuf,
    target: PathBuf,
}

impl Link {
    fn at(path: PathBuf, fs: &dyn FileSystem) -> Self {
        Self {
            target: PathBuf::from(fs.canonical_root(&path)),
            path,
        }
    }

    /// The event at the link that `event`, at or around its target, amounts
    /// to: a change below the target is the same change below the link, and
    /// the target, or a directory above it, appearing or leaving is the link's
    /// directory doing so.
    fn restate(&self, event: &FileEvent) -> Option<FileEvent> {
        let (path, kind): (&Path, fn(PathBuf) -> FileEvent) = match event {
            FileEvent::Created(p) => (p, FileEvent::Created),
            FileEvent::Modified(p) => (p, FileEvent::Modified),
            FileEvent::Deleted(p) => (p, FileEvent::Deleted),
            FileEvent::Rescan | FileEvent::Fault(_) => return None,
        };
        match path.strip_prefix(&self.target) {
            Ok(rest) => Some(kind(self.path.join(rest))),
            Err(_) if !matches!(event, FileEvent::Modified(_)) && self.target.starts_with(path) => {
                Some(kind(self.path.clone()))
            }
            Err(_) => None,
        }
    }
}

#[derive(Default)]
struct FreshState {
    /// Set by [`DirSQL::lock_index`]: nothing changes the index until
    /// [`DirSQL::unlock_index`].
    locked: bool,
}

/// The watcher the build keeps, given how starting it went.
fn watcher_at_build<W>(started: std::result::Result<W, notify::Error>) -> Result<Option<W>> {
    match started {
        Ok(watcher) => Ok(Some(watcher)),
        Err(e) if watcher::is_not_found(&e) => Ok(None),
        Err(e) => match WatchFault::of(&e) {
            WatchFault::Limit => Err((&WatchFault::Limit).into()),
            _ => Err(DirSqlError::watch(e)),
        },
    }
}

impl From<&WatchFault> for DirSqlError {
    fn from(fault: &WatchFault) -> Self {
        match fault {
            // What watchdog raises, so each binding raises its own OS error.
            WatchFault::Limit => Self::Io(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                OsError {
                    errno: 28,
                    code: "ENOSPC",
                    message: "inotify watch limit reached",
                },
            )),
            WatchFault::Overflow => Self::watch_msg(
                "the OS dropped file events, so the index no longer matches the files; rebuild it",
            ),
            WatchFault::Failed(message) => Self::watch_msg(message.clone()),
        }
    }
}

/// [`watch_scopes`] before the build has canonicalized each anchor.
fn build_watch_scopes(groups: &[AnchorGroup]) -> Vec<WatchScope> {
    groups
        .iter()
        .map(|group| WatchScope {
            root: PathBuf::from(canonical_root(&group.anchor)),
            ignore: group.matcher.clone(),
        })
        .collect()
}

fn watch_scopes(groups: &[AnchorGroup]) -> Vec<WatchScope> {
    groups
        .iter()
        .map(|group| WatchScope {
            root: group.watch_anchor.clone(),
            ignore: group.matcher.clone(),
        })
        .collect()
}

/// The tables sharing one anchor, matched against paths relative to it.
struct AnchorGroup {
    anchor: PathBuf,
    /// `anchor` canonicalized for the live watcher only: `notify` misbehaves
    /// on relative paths (it may deliver no events, or deliver them under the
    /// cwd-joined path so the relative prefix no longer strips). Literal
    /// fallback when canonicalization fails; `anchor` and the `path` column
    /// stay byte-for-byte as supplied.
    watch_anchor: PathBuf,
    matcher: TableMatcher,
    tables: Vec<String>,
}

/// Where each table's paths are anchored. A table without an entry anchors at
/// the index root.
#[derive(Clone)]
struct TableAnchors {
    index_root: PathBuf,
    by_table: HashMap<String, PathBuf>,
}

impl TableAnchors {
    fn at_root(index_root: &Path) -> Self {
        Self {
            index_root: index_root.to_path_buf(),
            by_table: HashMap::new(),
        }
    }

    fn of(&self, table: &str) -> &Path {
        self.by_table.get(table).unwrap_or(&self.index_root)
    }
}

#[derive(Clone)]
pub struct DirSQL {
    inner: Arc<DirSqlInner>,
}

/// Internal scan seam: the startup directory walk, injectable so unit tests
/// can observe whether it ran. The walk has no result a caller can tell apart
/// from not walking, so *whether* it ran takes a seam. Production always
/// passes [`scan_directory_reporting`]; the third argument is the walk's
/// running file count, which drives the progress line.
type ScanFn<'a> = &'a dyn Fn(
    &Path,
    &TableMatcher,
    &mut dyn FnMut(u64),
    &mut dyn FnMut(&Path, &str),
    &(dyn Fn(&Path) + Sync),
) -> Vec<Found>;

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
    /// config file's parent directory; the config's `[[table]]` globs anchor at
    /// that parent directory. To read `<root>/.dirsql.toml`, pass it
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
        self.query_ordered(sql).map(|result| result.rows)
    }

    /// [`query`](Self::query), plus the projection order in
    /// [`QueryResult::columns`]. A [`Row`] is a `HashMap`, so a caller that
    /// renders columns in the order the user asked for needs this instead.
    pub fn query_ordered(&self, sql: &str) -> Result<QueryResult> {
        self.query_ordered_with(sql, &Params::default())
    }

    /// [`query`](Self::query), binding `params` to the statement's
    /// placeholders. The values never enter the SQL text.
    pub fn query_with(&self, sql: &str, params: &Params) -> Result<Vec<Row>> {
        self.query_ordered_with(sql, params)
            .map(|result| result.rows)
    }

    /// [`query_ordered`](Self::query_ordered), binding `params` as
    /// [`query_with`](Self::query_with) does.
    pub fn query_ordered_with(&self, sql: &str, params: &Params) -> Result<QueryResult> {
        let _fresh = self.refresh()?;
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        db.query_ordered_with(sql, params).map_err(map_db_error)
    }

    /// [`query_ordered`](Self::query_ordered), abandoned once `cancelled` is
    /// set, releasing the index for the next query.
    #[cfg(feature = "cli")]
    pub(crate) fn query_ordered_until(
        &self,
        sql: &str,
        cancelled: Arc<AtomicBool>,
    ) -> Result<QueryResult> {
        let _fresh = self.refresh()?;
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        db.query_ordered_until(sql, cancelled).map_err(map_db_error)
    }

    /// Hold the index still until [`unlock_index`](Self::unlock_index): no
    /// query reads the disk and no watch batch applies until then. Waits for
    /// a read or batch already in progress, and for another holder.
    #[doc(hidden)]
    pub fn lock_index(&self) -> Result<()> {
        self.unlocked_fresh()?.locked = true;
        Ok(())
    }

    /// Release [`lock_index`](Self::lock_index).
    #[doc(hidden)]
    pub fn unlock_index(&self) -> Result<()> {
        self.inner.fresh.lock().map_err(DirSqlError::lock)?.locked = false;
        self.inner.unlocked.notify_all();
        Ok(())
    }

    fn unlocked_fresh(&self) -> Result<MutexGuard<'_, FreshState>> {
        let state = self.inner.fresh.lock().map_err(DirSqlError::lock)?;
        self.inner
            .unlocked
            .wait_while(state, |state| state.locked)
            .map_err(DirSqlError::lock)
    }

    /// Hold the index still until the returned guard drops. A watch batch
    /// releases the database between statements inside its transaction, so a
    /// query that took only the database could read it half applied.
    fn refresh(&self) -> Result<MutexGuard<'_, FreshState>> {
        let state = self.unlocked_fresh()?;
        if let Some(fault) = self.inner.fault.get() {
            return Err(fault.into());
        }
        self.raise_failed_file()?;
        Ok(state)
    }

    /// The first file failing since the build, as the error the build would
    /// have raised for it.
    fn raise_failed_file(&self) -> Result<()> {
        let failed = self.inner.failed_files.lock().map_err(DirSqlError::lock)?;
        match failed.iter().next() {
            Some(((table, path), message)) => Err(DirSqlError::OnFile {
                table: table.clone(),
                path: path.clone(),
                message: message.clone(),
            }),
            None => Ok(()),
        }
    }

    /// Record `table`'s read of `rel_path`: a failure stands until a later
    /// read of the file succeeds or the file leaves.
    fn record_read(&self, table: &str, rel_path: &str, failure: Option<&str>) {
        let Ok(mut failed) = self.inner.failed_files.lock() else {
            return;
        };
        let key = (table.to_string(), rel_path.to_string());
        match failure {
            Some(message) => {
                failed.insert(key, message.to_string());
            }
            None => {
                failed.remove(&key);
            }
        }
    }

    /// Forget the failures of `table`'s files at or beneath `rel_path`.
    fn forget_failures_under(&self, table: &str, rel_path: &str) {
        let Ok(mut failed) = self.inner.failed_files.lock() else {
            return;
        };
        let below = format!("{rel_path}/");
        failed.retain(|(t, path), _| t != table || (path != rel_path && !path.starts_with(&below)));
    }

    /// The directory the index is rooted at: where path-tables resolve.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// The tables a query can read by name, with their columns, straight from
    /// SQLite's catalog: empty tables included, no file read or table scanned.
    pub fn tables(&self) -> Result<Vec<TableInfo>> {
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        db.user_tables().map_err(map_db_error)
    }

    /// [`query_ordered`](Self::query_ordered) folding each row's cells, in
    /// projection order, into the state `start` makes from the projection,
    /// as they are read rather than collected first.
    pub fn query_each<S>(
        &self,
        sql: &str,
        start: impl FnOnce(&[String]) -> S,
        on_row: impl FnMut(&mut S, &[Value]),
    ) -> Result<S> {
        self.query_each_with(sql, &Params::default(), start, on_row)
    }

    /// [`query_each`](Self::query_each), binding `params` as
    /// [`query_with`](Self::query_with) does.
    pub fn query_each_with<S>(
        &self,
        sql: &str,
        params: &Params,
        start: impl FnOnce(&[String]) -> S,
        on_row: impl FnMut(&mut S, &[Value]),
    ) -> Result<S> {
        let _fresh = self.refresh()?;
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        db.query_each_with(sql, params, start, on_row)
            .map_err(map_db_error)
    }

    /// [`query_each`](Self::query_each) handing each row's cells as SQLite
    /// holds them, valid until the callback returns.
    #[cfg(feature = "cli")]
    pub(crate) fn query_each_ref<S>(
        &self,
        sql: &str,
        start: impl FnOnce(&[String]) -> S,
        on_row: impl FnMut(&mut S, &[rusqlite::types::ValueRef<'_>]),
    ) -> Result<S> {
        let _fresh = self.refresh()?;
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        db.query_each_ref(sql, &Params::default(), start, on_row)
            .map_err(map_db_error)
    }

    /// Attach to the watcher the build started, starting it if the build
    /// could not. Idempotent.
    #[doc(hidden)]
    pub fn attach_watcher(&self) -> Result<()> {
        self.require_named_tables()?;
        let mut guard = self.inner.watcher.lock().map_err(DirSqlError::lock)?;
        if guard.is_none() {
            let watcher = Watcher::scoped(self.watch_scopes()).map_err(DirSqlError::watch)?;
            *guard = Some(Arc::new(watcher));
        }
        Ok(())
    }

    fn require_named_tables(&self) -> Result<()> {
        if self.inner.groups.is_empty() {
            return Err(DirSqlError::watch_msg(
                "watching requires at least one named table: path-tables emit no watch events; define a table",
            ));
        }
        Ok(())
    }

    /// The trees the live watcher must cover: each table anchor, under the
    /// skip rules its tables' scan applies.
    fn watch_scopes(&self) -> Vec<WatchScope> {
        watch_scopes(&self.inner.groups)
    }

    /// Poll-based watch API. The first call attaches, as [`watch`](Self::watch)
    /// does; each call returns the [`RowEvent`]s of every change applied since
    /// the previous one, blocking up to `timeout` while there are none.
    /// Returns an empty batch if none arrive in time. Safe to call in a loop.
    ///
    /// Mutually exclusive with [`watch`](Self::watch): calling `watch` after
    /// `poll_events` (or vice versa) returns an error, because each would
    /// take the other's changes.
    pub fn poll_events(&self, timeout: Duration) -> Result<Vec<RowEvent>> {
        self.attach_poll()?;
        let polled = self.inner.polled.lock().map_err(DirSqlError::lock)?;
        let Some(rx) = polled.as_ref() else {
            return Ok(Vec::new());
        };
        let Ok(mut events) = rx.recv_timeout(timeout) else {
            return Ok(Vec::new());
        };
        events.extend(rx.try_iter().flatten());
        Ok(events)
    }

    /// Attach as [`poll_events`](Self::poll_events)' first call does, without
    /// waiting: changes applied from here on reach the next `poll_events`.
    #[doc(hidden)]
    pub fn attach_poll(&self) -> Result<()> {
        if self.inner.watch_thread_started.load(Ordering::SeqCst) {
            return Err(DirSqlError::watch_msg(
                "watch() is active; cannot mix with poll_events()",
            ));
        }
        self.inner.poll_used.store(true, Ordering::SeqCst);
        self.attach_watcher()?;
        let mut polled = self.inner.polled.lock().map_err(DirSqlError::lock)?;
        if polled.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            *self.inner.stream.lock().map_err(DirSqlError::lock)? = Some(Sink::Poll(tx));
            *polled = Some(rx);
        }
        Ok(())
    }

    /// Apply a batch of raw file events through the on_file/DB update
    /// pipeline.
    #[doc(hidden)]
    pub fn apply_file_events(&self, events: Vec<FileEvent>) -> Vec<RowEvent> {
        let _fresh = self.hold_index();
        self.apply_events(events)
    }

    /// Apply the watcher's queued events and hand them to the attached
    /// stream before a query can read them.
    fn apply_and_deliver(&self) {
        let _fresh = self.hold_index();
        self.deliver(self.apply_events(self.queued_file_events()));
    }

    /// The index, once no lock holds it.
    fn hold_index(&self) -> MutexGuard<'_, FreshState> {
        let fresh = self
            .inner
            .fresh
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.inner
            .unlocked
            .wait_while(fresh, |state| state.locked)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn apply_events(&self, events: Vec<FileEvent>) -> Vec<RowEvent> {
        if events.is_empty() {
            return Vec::new();
        }
        let held = self.take_held();
        if held.is_empty() {
            return match self.apply_batch(events) {
                Ok(out) => out,
                Err(rejected) => {
                    self.hold(rejected.events);
                    rejected.errors
                }
            };
        }
        // An orphan the held events left may be resolved by this batch, so
        // they go first, together. When they still can't land, this batch
        // is tried alone, so a change elsewhere is not held up behind them.
        let held_len = held.len();
        let mut rejected = match self.apply_batch(held.into_iter().chain(events).collect()) {
            Ok(out) => return out,
            Err(rejected) => rejected,
        };
        let fresh = rejected.events.split_off(held_len);
        self.hold(rejected.events);
        match self.apply_batch(fresh) {
            Ok(out) => rejected
                .errors
                .split_off(rejected.errors.len() - 1)
                .into_iter()
                .chain(out)
                .collect(),
            Err(fresh) => {
                self.hold(fresh.events);
                rejected.errors
            }
        }
    }

    /// Apply `events` in one transaction whose foreign keys are checked at its
    /// end.
    fn apply_batch(
        &self,
        events: Vec<FileEvent>,
    ) -> std::result::Result<Vec<RowEvent>, RejectedBatch> {
        if let Err(e) = self.with_db(|db| Ok(foreign_key::begin(db.conn())?)) {
            return Ok(vec![batch_error(e)]);
        }
        let (out, applied) = self.route_batch(events);
        match self.with_db(|db| foreign_key::commit(db.conn())) {
            Ok(()) => Ok(out),
            Err(e) => {
                let mut errors: Vec<RowEvent> = out
                    .into_iter()
                    .filter(|event| matches!(event, RowEvent::Error { .. }))
                    .collect();
                errors.push(batch_error(e));
                Err(RejectedBatch {
                    events: applied,
                    errors,
                })
            }
        }
    }

    /// Route `events` into the open transaction. Returns the row events and
    /// every file event routed.
    fn route_batch(&self, events: Vec<FileEvent>) -> (Vec<RowEvent>, Vec<FileEvent>) {
        let mut applied = Vec::new();
        let mut pending = PendingRefresh::default();
        let mut out = Vec::new();
        let mut rescan = false;
        for fe in events {
            applied.push(fe.clone());
            match fe {
                FileEvent::Rescan => {
                    self.record_fault(WatchFault::Overflow);
                    rescan = true;
                    continue;
                }
                FileEvent::Fault(fault) => {
                    self.record_fault(fault);
                    continue;
                }
                _ => {}
            }
            for fe in self.through_links(fe) {
                out.extend(self.route_file_event(fe, &mut pending));
            }
        }
        if rescan {
            out.extend(self.rescan(&mut pending));
        }
        out.extend(self.refresh_pending(pending));
        (out, applied)
    }

    /// The watcher has stopped seeing every change: from now on, every query
    /// fails with the first such fault.
    fn record_fault(&self, fault: WatchFault) {
        let _ = self.inner.fault.set(fault);
    }

    /// Keep `events` to replay with the next batch, once per path.
    fn hold(&self, events: Vec<FileEvent>) {
        let Ok(mut held) = self.inner.held.lock() else {
            return;
        };
        for event in events {
            if !held.contains(&event) {
                held.push(event);
            }
        }
    }

    /// The held events, each restated as the path's state now: a path that
    /// exists is re-read (or, for a directory, walked), and one that doesn't
    /// is deleted. A replay then can't undo a later change to the same path.
    fn take_held(&self) -> Vec<FileEvent> {
        let held = match self.inner.held.lock() {
            Ok(mut held) => std::mem::take(&mut *held),
            Err(_) => return Vec::new(),
        };
        let mut restated: Vec<FileEvent> = Vec::new();
        for event in held {
            let event = match event {
                FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => {
                    if self.inner.fs.is_file(&p).unwrap_or(false)
                        || self.inner.fs.is_dir(&p).unwrap_or(false)
                    {
                        FileEvent::Created(p)
                    } else {
                        FileEvent::Deleted(p)
                    }
                }
                other @ (FileEvent::Rescan | FileEvent::Fault(_)) => other,
            };
            if !restated.contains(&event) {
                restated.push(event);
            }
        }
        restated
    }

    fn with_db<T>(&self, f: impl FnOnce(&Db) -> std::result::Result<T, DbError>) -> Result<T> {
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        Ok(f(&db)?)
    }

    /// Channel-based watch API. The returned stream receives the [`RowEvent`]s
    /// of every change applied from now on. Intended for long-running Rust
    /// consumers (e.g. a CLI `watch` command). Can only be called once per
    /// `DirSQL` instance.
    ///
    /// Mutually exclusive with [`poll_events`](Self::poll_events).
    pub fn watch(&self) -> Result<WatchStream> {
        self.require_named_tables()?;
        if self.inner.poll_used.load(Ordering::SeqCst) {
            return Err(DirSqlError::watch_msg(
                "poll_events() already in use; cannot call watch()",
            ));
        }
        if self.inner.watch_thread_started.swap(true, Ordering::SeqCst) {
            return Err(DirSqlError::WatchAlreadyStarted);
        }
        self.attach_watcher()?;

        let (tx, rx) = unbounded();
        *self.inner.stream.lock().map_err(DirSqlError::lock)? = Some(Sink::Stream(tx));
        Ok(rx)
    }

    /// Hand `events` to the stream [`watch`](Self::watch) attached. Changes
    /// applied before it attached reach no stream.
    fn deliver(&self, events: Vec<RowEvent>) {
        let Ok(mut stream) = self.inner.stream.lock() else {
            return;
        };
        let delivered = match stream.as_ref() {
            None => return,
            Some(_) if events.is_empty() => return,
            Some(Sink::Stream(tx)) => events
                .into_iter()
                .all(|event| tx.unbounded_send(event).is_ok()),
            Some(Sink::Poll(tx)) => tx.send(events).is_ok(),
        };
        if !delivered {
            *stream = None;
        }
    }

    /// The events the watcher already holds, without waiting for more.
    fn queued_file_events(&self) -> Vec<FileEvent> {
        self.inner
            .watcher
            .lock()
            .ok()
            .and_then(|guard| guard.as_deref().map(Watcher::drain))
            .unwrap_or_default()
    }

    /// Process a single [`FileEvent`], mutating the DB and cache as needed.
    /// Operational errors become [`RowEvent::Error`] items in the returned
    /// vec (matching the semantics of the channel-based watch loop).
    #[cfg(test)]
    fn process_file_event(&self, event: FileEvent) -> Vec<RowEvent> {
        self.apply_file_events(vec![event])
    }

    /// Route one event: per-file tables are updated in place; a per-table
    /// hook's table is marked in `pending` for one re-run after the batch.
    fn route_file_event(&self, event: FileEvent, pending: &mut PendingRefresh) -> Vec<RowEvent> {
        let abs_path = match &event {
            FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => p.clone(),
            FileEvent::Rescan | FileEvent::Fault(_) => return Vec::new(),
        };
        let mut events = Vec::new();
        for (group, rel_path_buf) in self.groups_containing(&abs_path) {
            if group.matcher.is_ignored(&rel_path_buf) {
                continue;
            }
            if group.matcher.respects_gitignore()
                && scanner::is_gitignored_path(
                    &abs_path,
                    self.inner.fs.is_dir(&abs_path).unwrap_or(false),
                )
            {
                continue;
            }

            // A directory that appears whole (`mkdir`, or a populated directory
            // renamed into the tree) arrives as one event naming the directory and
            // none for the files already inside it, so the subtree is walked here.
            // Only on creation: a metadata change on a directory says nothing
            // about its files, and walking on every one would re-run `on_file`
            // across the subtree for a `chmod`.
            if matches!(event, FileEvent::Created(_))
                && self.inner.fs.is_dir(&abs_path).unwrap_or(false)
            {
                events.extend(self.index_subtree(group, &abs_path, pending, true));
                continue;
            }

            // Fan-out: dispatch the event to every table whose glob matches, and
            // concatenate the resulting row events. Cross-table event order is
            // unspecified. An `on_file` failure produces an error event for that
            // table only; the other matching tables still process the event.
            let matches = group.matcher.match_all(&rel_path_buf);
            let rel_path = scanner::to_slash(&rel_path_buf);

            for m in matches {
                if self.is_per_table(&m.table_name) {
                    let deleted = matches!(event, FileEvent::Deleted(_));
                    self.track_batch_file(&m.table_name, &abs_path, &rel_path, deleted);
                    pending.mark(&m.table_name, &rel_path);
                    continue;
                }
                match &event {
                    FileEvent::Deleted(_) => {
                        events.extend(self.handle_delete(&m.table_name, &rel_path));
                    }
                    FileEvent::Created(_) | FileEvent::Modified(_) => {
                        events.extend(self.handle_upsert(&m.table_name, &abs_path, &rel_path));
                    }
                    FileEvent::Rescan | FileEvent::Fault(_) => {}
                }
            }
            // The mirror of the walk above: a directory that leaves the tree
            // (`rm -r`, or a rename out of or within the root) arrives as one
            // event naming the directory and none for the files beneath it. It is
            // already gone, so the rows are the only record of what it held.
            if matches!(event, FileEvent::Deleted(_)) {
                events.extend(self.delete_subtree(group, &rel_path, pending));
            }
        }
        events
    }

    /// `event`, then the same change at each symlinked directory that
    /// reaches it. A deletion forgets the links it took with it.
    fn through_links(&self, event: FileEvent) -> Vec<FileEvent> {
        let Ok(mut links) = self.inner.links.lock() else {
            return vec![event];
        };
        let mut events = vec![event];
        for link in links.iter() {
            if let Some(restated) = link.restate(&events[0])
                && !events.contains(&restated)
            {
                events.push(restated);
            }
        }
        if let FileEvent::Deleted(gone) = &events[0] {
            links.retain(|link| !link.path.starts_with(gone));
        }
        events
    }

    /// Whether `dir` is reached through a known symlinked directory above it.
    fn below_a_link(&self, dir: &Path) -> bool {
        self.inner.links.lock().is_ok_and(|links| {
            links
                .iter()
                .any(|link| dir != link.path && dir.starts_with(&link.path))
        })
    }

    /// Remember `path` as a symlinked directory, replacing what it resolved
    /// to before. A path that resolves to itself is no link.
    fn register_link(&self, path: PathBuf) {
        let link = Link::at(path, self.inner.fs.as_ref());
        if link.target == link.path {
            return;
        }
        if let Ok(mut links) = self.inner.links.lock() {
            links.retain(|known| known.path != link.path);
            links.push(link);
        }
    }

    /// The anchor groups an event path falls under, each with the path
    /// relative to its anchor. Events arrive under the canonical
    /// `watch_anchor`, so that is stripped first; the user-supplied anchor
    /// (the already-canonical /absolute case) is the fallback. A path under
    /// no anchor is matched raw against every group.
    fn groups_containing(&self, abs_path: &Path) -> Vec<(&AnchorGroup, PathBuf)> {
        let under: Vec<(&AnchorGroup, PathBuf)> = self
            .inner
            .groups
            .iter()
            .filter_map(|group| {
                abs_path
                    .strip_prefix(&group.watch_anchor)
                    .or_else(|_| abs_path.strip_prefix(&group.anchor))
                    .ok()
                    .map(|rel| (group, rel.to_path_buf()))
            })
            .collect();
        if !under.is_empty() {
            return under;
        }
        self.inner
            .groups
            .iter()
            .map(|group| (group, abs_path.to_path_buf()))
            .collect()
    }

    fn is_per_table(&self, table: &str) -> bool {
        matches!(self.inner.on_file_map.get(table), Some(Hook::PerTable(_)))
    }

    /// Re-run every per-table hook `pending` names, once each, over the files
    /// its glob matches now, and replace that table's rows.
    fn refresh_pending(&self, pending: PendingRefresh) -> Vec<RowEvent> {
        let mut pending = pending.0;
        pending.sort_by_key(|(table, _)| declared_position(&self.inner.declared, table));
        let mut events = Vec::new();
        for (table, trigger) in pending {
            events.extend(self.refresh_table(&table, &trigger));
        }
        events
    }

    fn refresh_table(&self, table: &str, trigger: &str) -> Vec<RowEvent> {
        let Some(Hook::PerTable(hook)) = self.inner.on_file_map.get(table) else {
            return Vec::new();
        };
        let Some(group) = self
            .inner
            .groups
            .iter()
            .find(|group| group.tables.iter().any(|name| name == table))
        else {
            return Vec::new();
        };
        let abs_paths: Vec<PathBuf> = match self.inner.batch_files.lock() {
            Ok(files) => files
                .get(table)
                .map(|rel_paths| rel_paths.iter().map(|rel| group.anchor.join(rel)).collect())
                .unwrap_or_default(),
            Err(e) => return vec![error_event(Some(table), trigger, e.to_string())],
        };
        let strict = *self.inner.strict_map.get(table).unwrap_or(&false);
        let rows = Mutex::new(Vec::new());
        let take_rows = || std::mem::take(&mut *rows.lock().expect("hook rows lock"));
        let mut staged = None;
        let produced = hook(
            &mut abs_paths.into_iter(),
            &mut |chunk| rows.lock().expect("hook rows lock").extend(chunk),
            &mut || staged = Some(self.stage_table_rows(table, take_rows(), strict)),
        );
        let staged = match (produced, staged) {
            (Err(e), staged) => {
                if let Some(Ok(staged)) = staged {
                    let _ = foreign_key::discard(staged.db.conn());
                }
                return vec![error_event(Some(table), trigger, e.to_string())];
            }
            (Ok(()), Some(staged)) => staged,
            (Ok(()), None) => self.stage_table_rows(table, take_rows(), strict),
        };
        let changes = match staged {
            Ok(Staged { db, changes }) => match foreign_key::keep(db.conn()) {
                Ok(()) => changes,
                Err(e) => return vec![error_event(Some(table), trigger, e.to_string())],
            },
            Err(message) => return vec![error_event(Some(table), trigger, message)],
        };

        if changes.keyed {
            differ::diff_keyed(
                table,
                &changes.removed,
                &changes.updated,
                &changes.added,
                trigger,
            )
        } else {
            differ::diff_unordered(table, &changes.removed, &changes.added, trigger)
        }
    }

    /// Shape a per-table hook's rows and write them as `table`'s, held open
    /// under the database lock until the hook's exit keeps or discards them.
    fn stage_table_rows(
        &self,
        table: &str,
        raw_rows: Vec<infer::JsonRow>,
        strict: bool,
    ) -> std::result::Result<Staged<'_>, String> {
        let db = self.inner.db.lock().map_err(|e| e.to_string())?;
        let rows = shape_rows(&db, table, raw_rows, strict)?;
        let changes = foreign_key::stage(db.conn(), || {
            db.reconcile_batch_rows(table, BATCH_OWNER, &rows)
        })
        .map_err(|e| e.to_string())?;
        Ok(Staged { db, changes })
    }

    /// Record in the table's file set whether `abs_path` is, now, one of the
    /// files its per-table hook runs over. The set stays in walk order
    /// (component-wise byte order), so the hook sees the same argument list a
    /// walk of the tree would give it.
    fn track_batch_file(&self, table: &str, abs_path: &Path, rel_path: &str, deleted: bool) {
        let present = !deleted && self.inner.fs.is_file(abs_path).unwrap_or(false);
        let Ok(mut batch_files) = self.inner.batch_files.lock() else {
            return;
        };
        let files = batch_files.entry(table.to_string()).or_default();
        let at = files.binary_search_by(|file| Path::new(file).cmp(Path::new(rel_path)));
        match (present, at) {
            (true, Err(at)) => files.insert(at, rel_path.to_string()),
            (false, Ok(at)) => {
                files.remove(at);
            }
            _ => {}
        }
    }

    /// Delete the rows of every file recorded beneath `rel_dir`, in every
    /// table, as if each file had its own delete event.
    fn delete_subtree(
        &self,
        group: &AnchorGroup,
        rel_dir: &str,
        pending: &mut PendingRefresh,
    ) -> Vec<RowEvent> {
        let prefix = format!("{rel_dir}/");
        let in_group = |table: &str| group.tables.iter().any(|name| name == table);
        for table in &group.tables {
            self.forget_failures_under(table, rel_dir);
        }
        if let Ok(mut batch_files) = self.inner.batch_files.lock() {
            for (table, files) in batch_files.iter_mut() {
                if in_group(table) && files.iter().any(|file| file.starts_with(&prefix)) {
                    files.retain(|file| !file.starts_with(&prefix));
                    pending.mark(table, rel_dir);
                }
            }
        }
        let files = {
            let db = match self.inner.db.lock() {
                Ok(db) => db,
                Err(e) => return vec![error_event(None, rel_dir, e.to_string())],
            };
            match db.files_under(rel_dir) {
                Ok(files) => files,
                Err(e) => return vec![error_event(None, rel_dir, e.to_string())],
            }
        };
        let mut events = Vec::new();
        for (table, file_path) in files {
            if !in_group(&table) {
                continue;
            }
            events.extend(self.handle_delete(&table, &file_path));
        }
        events
    }

    /// Reconcile every table with the tree after the OS dropped events: walk
    /// each group's root again, drop the rows of files that are gone, re-read
    /// the files that are there, and rebuild each per-table hook's file list.
    fn rescan(&self, pending: &mut PendingRefresh) -> Vec<RowEvent> {
        if let Ok(mut failed) = self.inner.failed_files.lock() {
            failed.clear();
        }
        let owned = match self.inner.db.lock() {
            Ok(db) => db.owned_files(),
            Err(e) => return vec![error_event(None, "", e.to_string())],
        };
        let owned = match owned {
            Ok(owned) => owned,
            Err(e) => return vec![error_event(None, "", e.to_string())],
        };
        let mut events = Vec::new();
        for group in &self.inner.groups {
            let in_group = |table: &str| group.tables.iter().any(|name| name == table);
            for (table, file_path) in &owned {
                if in_group(table)
                    && !self
                        .inner
                        .fs
                        .is_file(&group.watch_anchor.join(file_path))
                        .unwrap_or(false)
                {
                    events.extend(self.handle_delete(table, file_path));
                }
            }
            for table in group.tables.iter().filter(|table| self.is_per_table(table)) {
                if let Ok(mut batch_files) = self.inner.batch_files.lock() {
                    batch_files.remove(table);
                }
                pending.mark(table, ".");
            }
            events.extend(self.index_subtree(group, &group.watch_anchor, pending, false));
        }
        events
    }

    /// Index every file beneath `dir` as if each had its own create event,
    /// using the scanner's walk so the watch path and the initial scan share
    /// one definition of what counts as a row. A `fresh` directory's files
    /// may still be being written: one that fails to parse is skipped, and
    /// read when its writer closes it.
    fn index_subtree(
        &self,
        group: &AnchorGroup,
        dir: &Path,
        pending: &mut PendingRefresh,
        fresh: bool,
    ) -> Vec<RowEvent> {
        let base = if dir.starts_with(&group.watch_anchor) {
            &group.watch_anchor
        } else {
            &group.anchor
        };
        if dir.starts_with(&group.watch_anchor) && !self.below_a_link(dir) {
            self.register_link(dir.to_path_buf());
        }
        let entered = Mutex::new(Vec::new());
        let found = self
            .inner
            .fs
            .scan_subtree(base, dir, &group.matcher, &|link| {
                entered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(link.to_path_buf());
            });
        for link in entered
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            self.register_link(link);
        }
        let mut events = Vec::new();
        for (path, table) in found {
            let rel_path = scanner::to_slash(path.strip_prefix(base).unwrap_or(&path));
            if self.is_per_table(&table) {
                self.track_batch_file(&table, &path, &rel_path, false);
                pending.mark(&table, &rel_path);
                continue;
            }
            events.extend(self.upsert(&table, &path, &rel_path, !fresh));
        }
        events
    }

    fn handle_delete(&self, table: &str, rel_path: &str) -> Vec<RowEvent> {
        self.forget_failures_under(table, rel_path);
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
            if let Err(e) =
                foreign_key::atomically(db.conn(), || db.delete_rows_by_file(table, rel_path))
            {
                return vec![error_event(Some(table), rel_path, e.to_string())];
            }
            old_rows
        };

        differ::diff(table, Some(&old_rows), None, rel_path)
    }

    fn handle_upsert(&self, table: &str, abs_path: &Path, rel_path: &str) -> Vec<RowEvent> {
        self.upsert(table, abs_path, rel_path, true)
    }

    /// [`handle_upsert`](Self::handle_upsert), reporting a parse failure only
    /// when `report_parse_error`.
    fn upsert(
        &self,
        table: &str,
        abs_path: &Path,
        rel_path: &str,
        report_parse_error: bool,
    ) -> Vec<RowEvent> {
        match self.read_file_rows(table, abs_path, rel_path, report_parse_error) {
            Ok(events) => {
                self.record_read(table, rel_path, None);
                events
            }
            Err(message) => {
                self.record_read(table, rel_path, Some(&message));
                vec![error_event(Some(table), rel_path, message)]
            }
        }
    }

    /// Run `table`'s hook over the file and write the rows it returns,
    /// failing with the message of whatever went wrong.
    fn read_file_rows(
        &self,
        table: &str,
        abs_path: &Path,
        rel_path: &str,
        report_parse_error: bool,
    ) -> std::result::Result<Vec<RowEvent>, String> {
        // The path may have vanished between the watcher event and now, or be a
        // directory (a `mkdir` under the root matches a `**/*` glob). Only
        // regular files become rows — mirror the initial scan, which skips
        // non-files.
        match self.inner.fs.is_file(abs_path) {
            Ok(true) => {}
            Ok(false) => return Ok(Vec::new()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        }

        let Some(Hook::PerFile(on_file)) = self.inner.on_file_map.get(table) else {
            return Ok(Vec::new());
        };

        let raw_rows = match on_file(&abs_path.to_string_lossy()) {
            Ok(r) => r,
            Err(_) if !report_parse_error => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };

        let strict = *self.inner.strict_map.get(table).unwrap_or(&false);

        // Shape, read the file's previous rows, and write the change under one
        // lock acquisition, so a concurrent event for the same file can't
        // interleave between the read and the write.
        let db = match self.inner.db.lock() {
            Ok(g) => g,
            Err(e) => return Err(e.to_string()),
        };
        let new_rows = match db.shape_value_rows(table, raw_rows, strict) {
            Ok(rows) => rows,
            Err(e) => return Err(e.to_string()),
        };
        let applied = foreign_key::atomically(db.conn(), || -> db::Result<_> {
            if let Some(changes) = db.reconcile_file_rows(table, rel_path, &new_rows)? {
                return Ok(differ::diff_keyed(
                    table,
                    &changes.removed,
                    &changes.updated,
                    &changes.added,
                    rel_path,
                ));
            }
            let old_rows = db.get_rows_by_file(table, rel_path)?;
            db.delete_rows_by_file(table, rel_path)?;
            let new_rows = db.as_stored(table, &new_rows)?.to_maps();
            for (i, row) in new_rows.iter().enumerate() {
                db.insert_row(table, row, rel_path, i)?;
            }
            Ok(differ::diff(
                table,
                Some(&old_rows),
                Some(&new_rows),
                rel_path,
            ))
        });
        applied.map_err(|e| e.to_string())
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
            functions: Vec::new(),
            persist: false,
            persist_path: None,
            poll_interval: DEFAULT_POLL_INTERVAL,
            hint_legacy_files_table: false,
            path_table_parser: None,
            no_ignore: false,
        };
        let prepared = Self::prepare_resolved(resolved)?;
        Self::finish_build_with_fs(prepared, fs, Progress::indexing())
    }

    /// Split-phase construction — part 1. Performs all I/O that is safe to run
    /// off the host's main thread: compiles the matcher, walks the directory,
    /// opens the database, creates transient tables when possible, and decides
    /// which files need re-parsing. Does **not** read file contents or invoke
    /// `on_file`.
    ///
    /// Pair with [`finish_build`](Self::finish_build) to complete construction
    /// on a thread where the `on_file` callback can safely execute (e.g. the
    /// JS main thread for the napi-rs TypeScript binding).
    #[doc(hidden)]
    pub fn prepare_resolved(resolved: ResolvedBuild) -> Result<PreparedBuild> {
        Self::prepare_resolved_with(resolved, &scan_directory_reporting)
    }

    fn prepare_resolved_with(resolved: ResolvedBuild, scan: ScanFn<'_>) -> Result<PreparedBuild> {
        Self::prepare_resolved_with_db(resolved, scan, Db::new)
    }

    fn prepare_resolved_with_db<F>(
        resolved: ResolvedBuild,
        scan: ScanFn<'_>,
        new_db: F,
    ) -> Result<PreparedBuild>
    where
        F: FnOnce() -> db::Result<Db> + Send,
    {
        Self::prepare_resolved_with_setup(resolved, scan, new_db, run_table_ddl)
    }

    fn prepare_resolved_with_setup<F, D>(
        resolved: ResolvedBuild,
        scan: ScanFn<'_>,
        new_db: F,
        ddl: D,
    ) -> Result<PreparedBuild>
    where
        F: FnOnce() -> db::Result<Db> + Send,
        D: FnOnce(&Db, &[Table]) -> Result<Vec<String>> + Send,
    {
        let ResolvedBuild {
            root,
            tables,
            ignore,
            extensions,
            functions,
            persist,
            persist_path,
            poll_interval,
            hint_legacy_files_table,
            path_table_parser,
            no_ignore,
        } = resolved;
        let (groups, anchors, table_names) = compile_groups(&tables, &ignore, &root)?;
        let groups = groups
            .into_iter()
            .map(|group| AnchorGroup {
                matcher: group.matcher.with_gitignore(!no_ignore),
                ..group
            })
            .collect::<Vec<_>>();

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

        // Zero tables means an empty `TableMatcher`, and `match_all` filters
        // its entries -- so the walk is guaranteed to return nothing. That is
        // the configless case (path-tables run their own per-statement scan),
        // where the walk was the bulk of startup.
        let links = Mutex::new(Vec::new());
        let (feeds, early) = if persist_ctx.is_none() {
            start_per_table_hooks(&tables)
        } else {
            (HashMap::new(), Vec::new())
        };
        // A watcher that cannot start yet (an anchor not created) does not
        // fail the build; watching starts it.
        let watcher = if table_names.is_empty() {
            None
        } else {
            watcher_at_build(Watcher::scoped(build_watch_scopes(&groups)))?
        };
        let (scanned, fresh_db) = if table_names.is_empty() {
            let fresh_db = if persist_ctx.is_none() {
                let mut db = new_db().map_err(map_db_error)?;
                configure_db_paths(
                    &mut db,
                    &root,
                    &ignore,
                    hint_legacy_files_table,
                    path_table_parser.as_deref(),
                    no_ignore,
                );
                Some((db, true, Vec::new()))
            } else {
                None
            };
            (Vec::new(), fresh_db)
        } else {
            std::thread::scope(|scope| {
                let db_task = persist_ctx.is_none().then(|| {
                    scope.spawn(|| {
                        let mut db = new_db().map_err(map_db_error)?;
                        configure_db_paths(
                            &mut db,
                            &root,
                            &ignore,
                            hint_legacy_files_table,
                            path_table_parser.as_deref(),
                            no_ignore,
                        );
                        let ddl_ready = extensions.is_empty() && functions.is_empty();
                        if ddl_ready {
                            let warnings = ddl(&db, &tables)?;
                            return Ok::<_, DirSqlError>((db, true, warnings));
                        }
                        Ok::<_, DirSqlError>((db, false, Vec::new()))
                    })
                });
                // The walk reports a running count rather than a fraction: it does
                // not know how many files there are until it has found them all.
                let mut progress = Progress::scanning();
                let (scanned, seen) = scan_groups(
                    &groups,
                    scan,
                    &mut progress,
                    &mut |path, table| {
                        if let Some(feed) = feeds.get(table) {
                            let _ = feed.send(path.to_path_buf());
                        }
                    },
                    &|link| {
                        links
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(link.to_path_buf());
                    },
                );
                drop(feeds);
                progress.finish(seen);
                for warning in empty_table_warnings(&tables, &anchors, &scanned) {
                    eprintln!("{warning}");
                }
                let fresh_db = db_task
                    .map(|task| task.join().expect("database creation panicked"))
                    .transpose()?;
                Ok::<_, DirSqlError>((scanned, fresh_db))
            })?
        };

        // When persist is enabled, files whose stat tuple matches the cache
        // (and that pass the racy-window check) are trusted instead of
        // re-parsed.
        let (scanned_files, deleted) = match &persist_ctx {
            None => (Scanned::Found(scanned), Vec::new()),
            Some(ctx) => {
                let (files, deleted) = reconcile_scan(scanned, ctx, &RealFs)?;
                (Scanned::Files(files), deleted)
            }
        };

        Ok(PreparedBuild {
            root,
            tables,
            extensions,
            functions,
            groups,
            anchors,
            ignore,
            scanned_files,
            fresh_db,
            early,
            hint_legacy_files_table,
            persist: persist_ctx.map(|ctx| PreparedPersist {
                db: ctx.db,
                deleted,
                meta: ctx.expected_meta,
                meta_current: ctx.meta_current,
                needs_sweep: ctx.needs_sweep,
            }),
            poll_interval,
            path_table_parser,
            no_ignore,
            links: links
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            watcher,
        })
    }

    /// Always empty: a hook that fails, or returns a row the table rejects,
    /// fails the build with [`DirSqlError::OnFile`] instead.
    pub fn scan_failures(&self) -> &[OnFileFailure] {
        &self.inner.scan_failures
    }

    /// Split-phase construction — part 2. Consumes the intermediate state from
    /// [`prepare_resolved`](Self::prepare_resolved): configures the prepared
    /// database, completes any deferred DDL, invokes each file's `on_file`
    /// callback, and inserts the resulting rows.
    ///
    /// Must be invoked on a thread where the `on_file` closures can safely
    /// run. For the napi-rs binding that is the JS main thread.
    #[doc(hidden)]
    pub fn finish_build(prepared: PreparedBuild) -> Result<Self> {
        let built = Self::finish_build_with_fs(prepared, Arc::new(RealFs), Progress::indexing())?;
        built.start_applier();
        Ok(built)
    }

    /// Apply the watcher's events on a thread of the instance's own, from now
    /// until it is dropped.
    fn start_applier(&self) {
        if self.inner.groups.is_empty() {
            return;
        }
        let inner = Arc::downgrade(&self.inner);
        thread::spawn(move || run_applier(inner));
    }

    /// Variant of [`finish_build`] taking the [`FileSystem`] to store on the
    /// instance and the ingest phase's [`Progress`] reporter. Production
    /// always passes `Arc::new(RealFs)` and a stderr reporter; unit tests
    /// inject fakes.
    pub(crate) fn finish_build_with_fs(
        prepared: PreparedBuild,
        fs: Arc<dyn FileSystem>,
        mut progress: Progress,
    ) -> Result<Self> {
        let PreparedBuild {
            root,
            tables,
            extensions,
            functions,
            groups,
            anchors,
            ignore,
            scanned_files,
            fresh_db,
            early,
            persist,
            poll_interval,
            hint_legacy_files_table,
            path_table_parser,
            no_ignore,
            links,
            watcher,
        } = prepared;

        let (mut db, fresh_ddl, ddl_warnings, persist_ready, needs_sweep) = match persist {
            Some(p) => (
                p.db,
                false,
                Vec::new(),
                Some((p.deleted, p.meta, p.meta_current)),
                p.needs_sweep,
            ),
            None => {
                let (db, ddl_ready, warnings) = fresh_db.expect("prepare opened the database");
                (db, ddl_ready, warnings, None, false)
            }
        };
        if persist_ready.is_some() {
            configure_db_paths(
                &mut db,
                &root,
                &ignore,
                hint_legacy_files_table,
                path_table_parser.as_deref(),
                no_ignore,
            );
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

        // Register declared functions after extensions so both are available
        // to queries. Registration is inert: no worker process exists until a
        // query's first call to the function.
        db.register_functions(&functions)
            .map_err(DirSqlError::sqlite)?;

        // Sweep a cache the reconcile rejected. It happens *here*, after the
        // extensions and functions are on the connection, rather than where
        // the cache was opened: the drop has to see the same modules the batch
        // that created these tables saw. A virtual table from a
        // `[[dirsql.extension]]` is undroppable otherwise -- `DROP TABLE`
        // answers `no such module`, and the cache stays wedged on every later
        // run until it is deleted by hand (#1008).
        if needs_sweep {
            drop_user_tables(db.conn()).map_err(DirSqlError::sqlite)?;
        }

        let mut on_file_map: HashMap<String, Hook> = HashMap::new();
        let mut strict_map: HashMap<String, bool> = HashMap::new();
        let mut ddl_map: HashMap<String, String> = HashMap::new();
        let declared: Vec<String> = tables.iter().map(|table| table.name.clone()).collect();

        let ddl_warnings = if fresh_ddl {
            ddl_warnings
        } else {
            run_table_ddl(&db, &tables)?
        };
        for warning in ddl_warnings {
            eprintln!("{warning}");
        }
        for table in tables {
            let table_name = table.name.clone();
            on_file_map.insert(table_name.clone(), table.hook);
            strict_map.insert(table_name.clone(), table.strict);
            ddl_map.insert(table_name, table.ddl);
        }

        // Begin one transaction for the entire ingest: deleted-file cleanup,
        // row deletes/inserts, and meta write. All drop together on any
        // error, leaving the cache exactly as it was before the build started.
        foreign_key::begin(db.conn()).map_err(DirSqlError::sqlite)?;

        // Drop cached rows for files that disappeared since the last cache
        // write. Trusted files need no work: their rows already live in the
        // on-disk SQLite.
        if let Some((deleted, _, _)) = persist_ready.as_ref() {
            for (rel_path, table_name) in deleted {
                db.delete_rows_by_file(table_name, rel_path)
                    .map_err(map_db_error)?;
                cache_delete_file(db.conn(), rel_path, table_name).map_err(DirSqlError::sqlite)?;
            }
        }

        let snapshot_ns = now_ns();
        let mut batch_files: HashMap<String, Vec<String>> = HashMap::new();

        // Grouping walks every scanned file, so it runs beside the early
        // hooks' inserts instead of ahead of them, and settles the early
        // tables' file lists there too.
        let early_tables: HashSet<String> = early.iter().map(|(name, _)| name.clone()).collect();
        let (grouped, streamed) = std::thread::scope(|scope| {
            let grouping = scope.spawn(|| {
                group_scanned(scanned_files.into_files(), &on_file_map)
                    .map(|(per_file, batches)| (per_file, split_streamed(batches, &early_tables)))
            });
            let streamed = stream_early(&db, early, &strict_map);
            (
                grouping.join().expect("grouping scanned files panicked"),
                streamed,
            )
        });
        streamed?;
        let (per_file, (mut batches, streamed)) = grouped?;
        batches.sort_by_key(|(table_name, _, _)| declared_position(&declared, table_name));

        // The phase a user actually waits on: one `on_file` round trip per
        // file or per table, plus whatever the table's DDL triggers on insert.
        // The count is known up front, so this one reports a fraction.
        let total_files = (per_file.len()
            + batches
                .iter()
                .map(|(_, _, files)| files.len())
                .sum::<usize>()
            + streamed.iter().map(|(_, rel)| rel.len()).sum::<usize>())
            as u64;
        let mut done: u64 = 0;
        for (table_name, rel_paths) in streamed {
            progress.update(done, Some(total_files));
            done += rel_paths.len() as u64;
            batch_files.insert(table_name, rel_paths);
        }
        for (
            ScannedFile {
                rel_path,
                table_name,
                stat,
                ..
            },
            on_file,
        ) in per_file
        {
            progress.update(done, Some(total_files));
            done += 1;
            let strict = *strict_map.get(&table_name).unwrap_or(&false);
            let abs_path = anchors.of(&table_name).join(&rel_path);
            let failed = |message: String| DirSqlError::OnFile {
                table: table_name.clone(),
                path: rel_path.clone(),
                message,
            };
            let raw_rows =
                on_file(&abs_path.to_string_lossy()).map_err(|e| failed(e.to_string()))?;

            // When updating an existing file in the persistent cache, drop
            // its old rows before inserting the new ones.
            if persist_ready.is_some() {
                db.delete_rows_by_file(&table_name, &rel_path)
                    .map_err(map_db_error)?;
            }
            let normalized = normalize_rows(&db, &table_name, raw_rows, strict).map_err(failed)?;
            for (row_index, row) in normalized.iter().enumerate() {
                db.insert_row(&table_name, row, &rel_path, row_index)
                    .map_err(|e| failed(e.to_string()))?;
            }

            // Update the persistent file index after a successful parse.
            if persist_ready.is_some()
                && let Some(stat) = stat.as_ref()
            {
                let hash = hash_file(&anchors.of(&table_name).join(&rel_path)).ok();
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

        for (table_name, hook, mut files) in batches {
            progress.update(done, Some(total_files));
            done += files.len() as u64;
            let strict = *strict_map.get(&table_name).unwrap_or(&false);
            let (rel_paths, abs_paths) = batch_paths(&mut files, persist_ready.is_some());
            // A cached table is current only when every file it was run over
            // is, and none has gone: the command saw them all at once, so any
            // change re-runs it over all of them.
            let current = persist_ready.as_ref().is_some_and(|(deleted, _, _)| {
                files.iter().all(|f| f.trusted) && !deleted.iter().any(|(_, t)| *t == table_name)
            });
            if !current {
                let table_root = anchors.of(&table_name);
                let production = start_hook(hook, abs_paths.into_iter(), Turn::default());
                let outcome = stream_table_rows(&db, &table_name, production, strict)
                    .map_err(map_db_error)?;
                match outcome {
                    Ok(()) => {
                        if persist_ready.is_some() {
                            for file in &files {
                                let Some(stat) = file.stat.as_ref() else {
                                    continue;
                                };
                                let hash = hash_file(&table_root.join(&file.rel_path)).ok();
                                upsert_file(
                                    db.conn(),
                                    &file.rel_path,
                                    &table_name,
                                    stat,
                                    hash.as_ref(),
                                    snapshot_ns,
                                )
                                .map_err(DirSqlError::sqlite)?;
                            }
                        }
                    }
                    Err(message) => {
                        return Err(DirSqlError::TableCommand {
                            name: table_name,
                            message,
                        });
                    }
                }
            }
            batch_files.insert(table_name, rel_paths);
        }

        progress.finish(total_files);

        // Write the meta block last; ingest and meta commit atomically in the
        // single transaction opened above. A crash mid-build leaves the cache
        // exactly as it was before the build started (detected via meta on
        // next startup).
        if let Some((_, meta, current)) = persist_ready.as_ref()
            && !current
        {
            write_meta(db.conn(), meta).map_err(DirSqlError::sqlite)?;
        }

        let mut groups = groups;
        for group in &mut groups {
            group.watch_anchor = PathBuf::from(fs.canonical_root(&group.anchor));
        }
        let links = links
            .into_iter()
            .map(|path| {
                let path = groups
                    .iter()
                    .find_map(|group| {
                        path.strip_prefix(&group.anchor)
                            .ok()
                            .map(|rel| group.watch_anchor.join(rel))
                    })
                    .unwrap_or(path);
                Link::at(path, fs.as_ref())
            })
            .collect();

        let built = Self {
            inner: Arc::new(DirSqlInner {
                db: Mutex::new(db),
                root,
                groups,
                on_file_map,
                strict_map,
                declared,
                batch_files: Mutex::new(batch_files),
                held: Mutex::new(Vec::new()),
                fault: OnceLock::new(),
                scan_failures: Vec::new(),
                watcher: Mutex::new(watcher.map(Arc::new)),
                poll_used: AtomicBool::new(false),
                watch_thread_started: AtomicBool::new(false),
                stream: Mutex::new(None),
                polled: Mutex::new(None),
                poll_interval,
                fs,
                fresh: Mutex::new(FreshState::default()),
                unlocked: Condvar::new(),
                links: Mutex::new(links),
                failed_files: Mutex::new(BTreeMap::new()),
            }),
        };
        built.apply_queued_at_build()?;
        Ok(built)
    }

    /// Apply what the watcher queued while the build walked, once, inside the
    /// build's transaction, then commit it with one foreign-key check.
    fn apply_queued_at_build(&self) -> Result<()> {
        let (out, _) = self.route_batch(self.queued_file_events());
        if let Some(RowEvent::Error {
            table,
            file_path,
            error,
        }) = out
            .into_iter()
            .find(|event| matches!(event, RowEvent::Error { .. }))
        {
            return Err(match table {
                Some(table) => DirSqlError::OnFile {
                    table,
                    path: scanner::to_slash(&file_path),
                    message: error,
                },
                None => DirSqlError::watch_msg(error),
            });
        }
        let db = self.inner.db.lock().map_err(DirSqlError::lock)?;
        foreign_key::commit(db.conn()).map_err(map_db_error)
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
///     .table(Table::new("t", "CREATE TABLE t (x TEXT)", "*.json", |_, _| vec![]))
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
    no_ignore: bool,
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
    /// process cwd. Its `[[table]]` globs anchor at the config's parent directory
    /// (or the literal prefix of an absolute / `~/` glob), and its `ignore`
    /// applies to those tables only. Relative `persist_path` / `[[dirsql.extension]]` paths still
    /// resolve against the config's parent directory.
    ///
    /// Call repeatedly to load several configs: their `[[table]]`, `ignore`, and
    /// `[[dirsql.extension]]` entries accumulate in call order, and each config's
    /// `on-file` hooks run from that config file's own directory. A single call
    /// is identical to before.
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
    /// the default location in the platform cache directory; `Some(path)` writes it to `path`.
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

    /// Disable `.gitignore` respect in path-table scans (the CLI's
    /// `--no-ignore`). Path-tables honor `.gitignore` files by default;
    /// with this set they scan the full tree. The built-in skips
    /// (`node_modules`/`.git`) and configured [`ignore`](Self::ignore)
    /// patterns apply either way.
    pub fn no_ignore(mut self, no_ignore: bool) -> Self {
        self.no_ignore = no_ignore;
        self
    }

    fn resolve(self) -> Result<ResolvedBuild> {
        let DirSQLBuilder {
            root: explicit_root,
            mut tables,
            ignore,
            mut extensions,
            config_paths,
            suppress_config_extensions,
            persist,
            persist_path,
            poll_interval,
            path_table_parser,
            no_ignore,
        } = self;

        // The index root is an operational fact owned by the runner: the
        // explicit `.root(...)` when given, else the process cwd. The config
        // file's own location plays no part in root derivation (#540).
        let root = match explicit_root {
            Some(explicit) => std::path::absolute(explicit).map_err(DirSqlError::config)?,
            None => std::env::current_dir().map_err(DirSqlError::config)?,
        };

        // The config layer is an ordered list of entries, each carrying its
        // loaded `config` and its `config_dir` (where `on-file` hooks run and
        // the base for extension path resolution). Configs accumulate in
        // `.config()` call order; a single entry makes the in-order merge
        // below byte-for-byte identical to a single pass.
        let mut config_entries: Vec<ResolvedConfigEntry> = Vec::new();
        let mut functions: Vec<ResolvedFunction> = Vec::new();
        let mut function_sources: HashMap<String, PathBuf> = HashMap::new();
        for cfg_path in &config_paths {
            let mut cfg = config::load_config(cfg_path).map_err(DirSqlError::config)?;

            let cfg_parent = std::path::absolute(cfg_path)
                .map_err(DirSqlError::config)?
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));

            resolve_functions(
                std::mem::take(&mut cfg.functions),
                cfg_path,
                &cfg_parent,
                &mut function_sources,
                &mut functions,
            )?;

            config_entries.push(ResolvedConfigEntry {
                config: cfg,
                config_dir: cfg_parent,
            });
        }

        for entry in config_entries {
            let ResolvedConfigEntry {
                config: cfg,
                config_dir: cfg_parent,
            } = entry;

            let cfg_tables = build_tables_from_config(&cfg, &cfg_parent)?;
            tables.extend(cfg_tables);

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

        if let Some(message) = path_table_parser
            .as_deref()
            .and_then(on_file::path_placeholder_rejection)
        {
            return Err(DirSqlError::PathPlaceholder(message));
        }

        Ok(ResolvedBuild {
            root,
            tables,
            ignore,
            extensions,
            functions,
            persist,
            persist_path,
            poll_interval: poll_interval.unwrap_or(DEFAULT_POLL_INTERVAL),
            hint_legacy_files_table,
            path_table_parser,
            no_ignore,
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
/// merges over. Carries the loaded `config` and its `config_dir` (the config
/// file's parent -- where `on-file` hooks run and the base for extension path
/// resolution). The index root is owned by the runner (#540), not derived per
/// config entry.
struct ResolvedConfigEntry {
    config: config::Config,
    config_dir: PathBuf,
}

/// Fully-resolved builder inputs: the result of merging programmatic
/// settings with values loaded from a `.dirsql.toml` config file.
#[doc(hidden)]
pub struct ResolvedBuild {
    pub root: PathBuf,
    pub tables: Vec<Table>,
    pub ignore: Vec<String>,
    pub extensions: Vec<Extension>,
    /// Worker-backed SQL scalar functions to register at startup
    /// (`[[dirsql.function]]`), already resolved per source config.
    pub functions: Vec<ResolvedFunction>,
    pub persist: bool,
    pub persist_path: Option<PathBuf>,
    pub poll_interval: Duration,
    /// Arms the missing-`files` path-table hint; see [`is_configless`].
    pub hint_legacy_files_table: bool,
    /// When set (the CLI's `--on-file`), every path-table is minted over this
    /// parser command instead of the stat columns.
    pub path_table_parser: Option<String>,
    /// When set (the CLI's `--no-ignore`), path-table scans do not respect
    /// `.gitignore` files.
    pub no_ignore: bool,
}

/// A single file discovered during [`DirSQL::prepare_resolved`]: its
/// root-relative path, the table it belongs to, and the filesystem stat
/// tuple captured during the scan (when persist is on).
#[doc(hidden)]
pub struct ScannedFile {
    pub abs_path: PathBuf,
    pub rel_path: String,
    pub table_name: String,
    pub stat: Option<FileStat>,
    /// Whether the persistent cache's rows for this file are current, so a
    /// per-file hook skips it. A per-table hook re-runs over every file as
    /// soon as any one of its files is not.
    pub trusted: bool,
}

/// The files the startup scan found: as the walk found them, or already
/// reconciled against the persistent cache.
enum Scanned {
    Found(Vec<Found>),
    Files(Vec<ScannedFile>),
}

impl Scanned {
    /// Every file as a [`ScannedFile`]. A fresh walk's are made here rather
    /// than in prepare, so the copy runs off the thread that inserts rows.
    fn into_files(self) -> Vec<ScannedFile> {
        match self {
            Self::Found(found) => found
                .into_iter()
                .map(|found| ScannedFile {
                    abs_path: found.path,
                    rel_path: found.rel_path,
                    table_name: found.table_name,
                    stat: None,
                    trusted: false,
                })
                .collect(),
            Self::Files(files) => files,
        }
    }
}

/// Intermediate state produced by [`DirSQL::prepare_resolved`] and consumed
/// by [`DirSQL::finish_build`]. Opaque on purpose; fields are only visible
/// to the in-workspace bindings.
#[doc(hidden)]
pub struct PreparedBuild {
    root: PathBuf,
    tables: Vec<Table>,
    groups: Vec<AnchorGroup>,
    anchors: TableAnchors,
    /// SQLite extensions to load onto the connection before any table DDL.
    extensions: Vec<Extension>,
    /// Worker-backed SQL scalar functions to register after extensions load.
    functions: Vec<ResolvedFunction>,
    /// The configured skip rules, carried through so path-table scans apply
    /// the same ones declared tables do.
    ignore: Vec<String>,
    scanned_files: Scanned,
    /// The transient database opened during the scan, whether its tables'
    /// DDL already ran, and that DDL's warnings. `None` when persisting.
    fresh_db: Option<(Db, bool, Vec<String>)>,
    /// Per-table hooks already running over the paths the scan found, by table.
    early: Vec<(String, Production)>,
    persist: Option<PreparedPersist>,
    poll_interval: Duration,
    hint_legacy_files_table: bool,
    path_table_parser: Option<String>,
    no_ignore: bool,
    /// The symlinked directories the walk entered.
    links: Vec<PathBuf>,
    /// Started before the walk, so it holds every change the walk may miss.
    watcher: Option<Watcher>,
}

#[doc(hidden)]
pub struct PreparedPersist {
    db: Db,
    deleted: Vec<(String, String)>,
    meta: HashMap<String, String>,
    /// Whether the cache already holds exactly `meta`. When it does the meta
    /// write is skipped, so a run that changed nothing leaves the cache file
    /// byte-for-byte alone.
    meta_current: bool,
    /// Whether the stale cache still has to be swept. Carried across the split
    /// so the drop runs on a fully configured connection — see
    /// [`PersistContext::needs_sweep`].
    needs_sweep: bool,
}

struct PersistContext {
    db: Db,
    /// Cached file bookkeeping keyed by `(rel_path, table_name)` — a file may
    /// be cached under several tables under fan-out.
    cached: HashMap<(String, String), CachedFile>,
    expected_meta: HashMap<String, String>,
    /// Whether the on-disk meta already equals `expected_meta`.
    meta_current: bool,
    /// Whether the cache holds tables from an incompatible prior run that must
    /// be dropped before the DDL batch re-creates them. The drop itself is
    /// deferred to `finish_build`, which runs it once the connection carries
    /// the configured extensions and functions: dropping a virtual table needs
    /// its module registered, and an extension-provided module exists only on
    /// a connection that loaded that extension (#1008).
    needs_sweep: bool,
}

type GroupDraft = (PathBuf, Vec<String>, Vec<(String, String)>, Vec<String>);

fn configure_db_paths(
    db: &mut Db,
    root: &Path,
    ignore: &[String],
    hint_legacy_files_table: bool,
    path_table_parser: Option<&str>,
    no_ignore: bool,
) {
    db.set_path_table_root(root.to_path_buf());
    db.set_hint_legacy_files_table(hint_legacy_files_table);
    db.add_path_table_ignore(ignore.to_vec());
    db.set_path_table_gitignore(!no_ignore);
    if let Some(command) = path_table_parser {
        db.set_path_table_parser(command.to_owned());
    }
}

fn run_table_ddl(db: &Db, tables: &[Table]) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    for table in tables {
        let table_name = &table.name;
        let warm = catalog_entry(db, table_name)?.is_some_and(|entry| entry.kind == "table");
        if !warm {
            let before = db::user_table_names(db.conn()).map_err(DirSqlError::sqlite)?;
            db.create_table(table_name, &table.ddl)
                .map_err(|source| DirSqlError::TableDdl {
                    name: table_name.clone(),
                    source,
                })?;
            let created = db::user_table_names(db.conn())
                .map_err(DirSqlError::sqlite)?
                .into_iter()
                .filter(|name| !before.contains(name))
                .collect();
            let entry = catalog_entry(db, table_name)?;
            if let Some(warning) = classify_declared_table(table_name, entry, created)? {
                warnings.push(warning);
            }
        }
    }
    Ok(warnings)
}

/// Walk every group's anchor in turn, reporting one running count across all of
/// them. Returns the matches and the total count.
fn scan_groups(
    groups: &[AnchorGroup],
    scan: ScanFn<'_>,
    progress: &mut Progress,
    on_match: &mut dyn FnMut(&Path, &str),
    on_link: &(dyn Fn(&Path) + Sync),
) -> (Vec<Found>, u64) {
    let mut seen = 0;
    let mut scanned = Vec::new();
    for group in groups {
        let before = seen;
        scanned.extend(scan(
            &group.anchor,
            &group.matcher,
            &mut |count| {
                seen = before + count;
                progress.update(seen, None);
            },
            on_match,
            on_link,
        ));
    }
    (scanned, seen)
}

fn compile_groups(
    tables: &[Table],
    ignore_patterns: &[String],
    root: &Path,
) -> Result<(Vec<AnchorGroup>, TableAnchors, Vec<String>)> {
    let mut seen: HashMap<String, ()> = HashMap::with_capacity(tables.len());
    let mut names = Vec::with_capacity(tables.len());
    let mut anchors = TableAnchors::at_root(root);
    let mut grouped: Vec<GroupDraft> = Vec::new();
    for table in tables {
        let table_name = table.name.clone();
        // Validate up front so a poisoned name from a stored cache or a
        // declared `name` carrying SQL can't propagate into `on_file_map`,
        // `strict_map`, or any format!()-built SQL down the line.
        crate::db::validate_identifier(&table_name).map_err(map_db_error)?;
        if seen.insert(table_name.clone(), ()).is_some() {
            return Err(DirSqlError::DuplicateTable(table_name));
        }
        let anchor = match &table.anchor {
            Some(anchor) => {
                anchors.by_table.insert(table_name.clone(), anchor.clone());
                anchor.clone()
            }
            None => root.to_path_buf(),
        };
        let mapping = (
            path_table::directory_as_glob(&table.glob, &anchor, &|p| p.is_dir()),
            table_name.clone(),
        );
        let named = path_table::named_directory(&table.glob, &anchor, &|p| p.is_dir());
        match grouped
            .iter_mut()
            .find(|(a, i, _, _)| *a == anchor && *i == table.ignore)
        {
            Some((_, _, mappings, dirs)) => {
                mappings.push(mapping);
                dirs.extend(named);
            }
            None => grouped.push((
                anchor,
                table.ignore.clone(),
                vec![mapping],
                named.into_iter().collect(),
            )),
        }
        names.push(table_name);
    }

    let mut groups = Vec::with_capacity(grouped.len());
    for (anchor, scoped_ignore, mappings, named_dirs) in grouped {
        let ignore_refs: Vec<&str> = ignore_patterns
            .iter()
            .chain(&scoped_ignore)
            .map(String::as_str)
            .collect();
        let mapping_refs: Vec<(&str, &str)> = mappings
            .iter()
            .map(|(g, n)| (g.as_str(), n.as_str()))
            .collect();
        let matcher = TableMatcher::new(&mapping_refs, &ignore_refs)
            .map_err(DirSqlError::matcher)?
            .with_named_dirs(&named_dirs);
        groups.push(AnchorGroup {
            watch_anchor: anchor.clone(),
            anchor,
            matcher,
            tables: mappings.into_iter().map(|(_, name)| name).collect(),
        });
    }
    Ok((groups, anchors, names))
}

/// One stderr line per config table that matched no file in the scan, naming
/// the table, its glob and the directory the glob was matched under.
fn empty_table_warnings(
    tables: &[Table],
    anchors: &TableAnchors,
    scanned: &[Found],
) -> Vec<String> {
    tables
        .iter()
        .filter(|table| table.warn_when_empty)
        .filter(|table| !scanned.iter().any(|found| found.table_name == table.name))
        .map(|table| {
            format!(
                "dirsql: table '{}': glob '{}' matched no files under {}",
                table.name,
                table.glob,
                anchors.of(&table.name).display()
            )
        })
        .collect()
}

/// Open (or create) the persistent SQLite cache and read its meta. If the
/// meta is missing or incompatible with the current build, the resulting
/// [`PersistContext`] carries an empty file index and asks for a sweep, so
/// the rest of the pipeline treats every file as new.
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
        HashMap::new()
    };

    Ok(PersistContext {
        db,
        cached,
        // Compatible means every expected key matches; equal length makes it
        // equality, which is what lets the write be skipped outright.
        meta_current: compatible && cached_meta.len() == expected_meta.len(),
        expected_meta,
        needs_sweep: !compatible,
    })
}

/// Internal filesystem seam: every effectful filesystem read in the
/// persist/reconcile and watch-upsert paths goes through this trait so unit
/// tests can inject a deterministic double. Production always uses [`RealFs`].
trait FileSystem: Send + Sync {
    fn stat(&self, path: &Path) -> std::io::Result<FileStat>;
    /// Whether `path` is a regular file. `Err(NotFound)` when it doesn't exist.
    fn is_file(&self, path: &Path) -> std::io::Result<bool>;
    /// Whether `path` is a directory. `Err(NotFound)` when it doesn't exist.
    fn is_dir(&self, path: &Path) -> std::io::Result<bool>;
    /// The scanner's walk over the subtree at `dir`, matched relative to
    /// `root`, telling `on_link` each symlinked directory it enters.
    fn scan_subtree(
        &self,
        root: &Path,
        dir: &Path,
        matcher: &TableMatcher,
        on_link: &(dyn Fn(&Path) + Sync),
    ) -> Vec<(PathBuf, String)>;
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

    fn is_dir(&self, path: &Path) -> std::io::Result<bool> {
        std::fs::metadata(path).map(|m| m.is_dir())
    }

    fn scan_subtree(
        &self,
        root: &Path,
        dir: &Path,
        matcher: &TableMatcher,
        on_link: &(dyn Fn(&Path) + Sync),
    ) -> Vec<(PathBuf, String)> {
        scanner::scan_subtree_reporting(root, dir, matcher, on_link)
    }

    fn hash(&self, path: &Path) -> std::io::Result<[u8; 32]> {
        hash_file(path)
    }

    fn canonical_root(&self, root: &Path) -> String {
        canonical_root(root)
    }
}

/// Decide which files are trusted and which need re-parsing, in scan order,
/// and which were removed since the last cache write.
#[allow(clippy::type_complexity)]
fn reconcile_scan(
    scanned: Vec<Found>,
    ctx: &PersistContext,
    fs: &dyn FileSystem,
) -> Result<(Vec<ScannedFile>, Vec<(String, String)>)> {
    let mut files = Vec::with_capacity(scanned.len());
    // Keyed by (rel_path, table_name): under fan-out one file may be scanned
    // for several tables, and each pair is trusted/deleted independently.
    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::with_capacity(scanned.len());

    for Found {
        path,
        rel_path,
        table_name,
    } in scanned
    {
        seen.insert((rel_path.clone(), table_name.clone()));

        let stat = fs.stat(&path)?;

        let cached = ctx.cached.get(&(rel_path.clone(), table_name.clone()));
        let trusted = cached.is_some_and(|c| {
            is_trusted(
                &c.stat,
                c.content_hash.as_ref(),
                c.snapshot_ns,
                &stat,
                || fs.hash(&path).ok(),
            )
        });

        files.push(ScannedFile {
            abs_path: path,
            rel_path,
            table_name,
            stat: Some(stat),
            trusted,
        });
    }

    let mut deleted = Vec::new();
    for (rel_path, table_name) in ctx.cached.keys() {
        if !seen.contains(&(rel_path.clone(), table_name.clone())) {
            deleted.push((rel_path.clone(), table_name.clone()));
        }
    }

    Ok((files, deleted))
}

/// What SQLite's catalog holds under `name`, if anything.
fn catalog_entry(db: &Db, name: &str) -> Result<Option<db::CatalogEntry>> {
    db::table_catalog_entry(db.conn(), name).map_err(DirSqlError::sqlite)
}

/// Settle what a table's `ddl` batch produced for its declared `name`, given
/// SQLite's catalog and the tables the batch added. `created` is a before/after
/// catalog diff, so a `name` that never appeared can say what did.
///
/// `Ok(None)` accepts the table, `Ok(Some(warning))` accepts it with a line for
/// stderr, and `Err` fails the load before any file is ingested. Pure: the
/// caller reads the catalog and prints the warning.
fn classify_declared_table(
    name: &str,
    entry: Option<db::CatalogEntry>,
    mut created: Vec<String>,
) -> Result<Option<String>> {
    let Some(entry) = entry else {
        // The catalog hands these back in drop-safe order (virtuals first);
        // sort so the message reads as a list rather than as an artifact of
        // how the sweep needs them.
        created.sort();
        return Err(DirSqlError::TableNotCreated {
            name: name.to_string(),
            created,
        });
    };
    // A dirsql table is the per-file row table rows are inserted into; an
    // extension-backed virtual table is not one. The batch may still create as
    // many as it likes alongside it.
    if entry.kind == "virtual" {
        return Err(DirSqlError::TableIsVirtual {
            name: name.to_string(),
        });
    }
    // A `WITHOUT ROWID` table has no rowid, so `last_insert_rowid()` cannot
    // identify the inserted row and the `_dirsql_internal_rows.rowid_ref`
    // mapping would be meaningless. Warn; the table is still usable.
    if entry.without_rowid {
        return Ok(Some(format!(
            "dirsql: table `{name}` is declared WITHOUT ROWID; internal row \
             bookkeeping relies on rowid and WITHOUT ROWID tables will be \
             rejected in a future release"
        )));
    }
    Ok(None)
}

fn map_db_error(e: DbError) -> DirSqlError {
    match e {
        DbError::WriteForbidden => DirSqlError::WriteForbidden,
        other => DirSqlError::Core(other),
    }
}

/// The per-table hooks one batch of file events touched, each with the first
/// path that touched it. Marked once per table however many events arrive,
/// which is what keeps a burst of writes to one re-run.
#[derive(Default)]
struct PendingRefresh(Vec<(String, String)>);

impl PendingRefresh {
    fn mark(&mut self, table: &str, trigger: &str) {
        if !self.0.iter().any(|(t, _)| t == table) {
            self.0.push((table.to_string(), trigger.to_string()));
        }
    }
}

/// Normalize every row before any is inserted, so a hook that got one row
/// wrong fails as a unit rather than leaving half its rows behind.
fn normalize_rows(
    db: &Db,
    table: &str,
    raw_rows: Vec<Row>,
    strict: bool,
) -> std::result::Result<Vec<Row>, String> {
    db.normalize_rows(table, raw_rows, strict)
        .map_err(|e| e.to_string())
}

/// Shape every row before any is inserted, so a hook that got one row wrong
/// fails as a unit rather than leaving half its rows behind.
fn shape_rows(
    db: &Db,
    table: &str,
    raw_rows: Vec<infer::JsonRow>,
    strict: bool,
) -> std::result::Result<ShapedRows, String> {
    db.shape_rows(table, raw_rows, strict)
        .map_err(|e| e.to_string())
}

/// Replace a per-table hook's rows under [`BATCH_OWNER`], storing each chunk
/// the hook emits while the hook, on its own thread, goes on producing the
/// next. Chunks are shaped on a thread of their own, in order, so shaping one
/// overlaps inserting the last. The caller's open transaction keeps a failure
/// from being seen: the outer `Err` is SQLite's, the inner one the hook's or a
/// row's. A hook failure outranks a row that failed to shape.
fn stream_table_rows(
    db: &Db,
    table: &str,
    production: Production,
    strict: bool,
) -> db::Result<std::result::Result<(), String>> {
    if db.owns_rows(table, BATCH_OWNER)? {
        return refill_table_rows(db, table, production, strict);
    }
    let columns = db.get_table_columns(table)?;
    let Production { chunks, handle } = production;
    let (shaped_tx, shaped_rx) = std::sync::mpsc::channel();
    let shaper = std::thread::spawn({
        let table = table.to_string();
        move || {
            for chunk in chunks {
                let shaped = db::shape_json_rows(&table, columns.clone(), chunk, strict);
                if shaped_tx.send(shaped).is_err() {
                    return;
                }
            }
        }
    });
    let mut shaped = Ok(());
    for rows in shaped_rx {
        match rows {
            Ok(rows) => db.insert_rows(table, &rows, BATCH_OWNER)?,
            Err(e) => {
                shaped = Err(e.to_string());
                break;
            }
        }
    }
    shaper.join().expect("shaping a hook's rows panicked");
    let produced = handle.join().expect("per-table hook panicked");
    Ok(produced.and(shaped))
}

/// Make a per-table hook's rows already stored under [`BATCH_OWNER`] equal
/// the rows it produces now, gathered whole so they can be matched against
/// the stored ones. Nothing is written unless the hook and every row succeed.
fn refill_table_rows(
    db: &Db,
    table: &str,
    production: Production,
    strict: bool,
) -> db::Result<std::result::Result<(), String>> {
    let columns = db.get_table_columns(table)?;
    let Production { chunks, handle } = production;
    let mut rows = ShapedRows {
        columns: columns.clone(),
        cells: Vec::new(),
        emitted: vec![false; columns.len()],
    };
    let mut shaped = Ok(());
    for chunk in chunks {
        match db::shape_json_rows(table, columns.clone(), chunk, strict) {
            Ok(chunk) => {
                rows.cells.extend(chunk.cells);
                for (emitted, now) in rows.emitted.iter_mut().zip(chunk.emitted) {
                    *emitted |= now;
                }
            }
            Err(e) => {
                shaped = Err(e.to_string());
                break;
            }
        }
    }
    let produced = handle.join().expect("per-table hook panicked");
    let outcome = produced.and(shaped);
    if outcome.is_ok() {
        db.refill_batch_rows(table, BATCH_OWNER, &rows)?;
    }
    Ok(outcome)
}

/// A per-table hook's rows written but not yet kept.
struct Staged<'a> {
    db: std::sync::MutexGuard<'a, Db>,
    changes: db::RowChanges,
}

/// A per-table hook running on its own thread, its rows arriving as chunks.
struct Production {
    chunks: std::sync::mpsc::Receiver<Vec<infer::JsonRow>>,
    handle: std::thread::JoinHandle<std::result::Result<(), String>>,
}

/// A per-table hook's place in declaration order: it waits for the hook
/// before it to finish, and releases the next by dropping `done` as it ends.
#[derive(Default)]
struct Turn {
    after: Option<std::sync::mpsc::Receiver<()>>,
    done: Option<std::sync::mpsc::Sender<()>>,
}

fn start_hook(
    hook: Arc<OnFilesFn>,
    paths: impl Iterator<Item = PathBuf> + Send + 'static,
    turn: Turn,
) -> Production {
    let (sender, chunks) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let Turn { after, done: _done } = turn;
        if let Some(after) = after {
            let _ = after.recv();
        }
        let mut paths = paths.peekable();
        if paths.peek().is_none() {
            return Ok(());
        }
        hook(
            &mut paths,
            &mut |chunk| {
                let _ = sender.send(chunk);
            },
            &mut || {},
        )
        .map_err(|e| e.to_string())
    });
    Production { chunks, handle }
}

/// Start every per-table hook before the scan, so the first runs over its
/// paths as the scan finds them and each later one runs as soon as the one
/// declared before it finishes: the senders feed the hooks, and closing them
/// ends the paths.
#[allow(clippy::type_complexity)]
fn start_per_table_hooks(
    tables: &[Table],
) -> (
    HashMap<String, std::sync::mpsc::Sender<PathBuf>>,
    Vec<(String, Production)>,
) {
    let mut feeds = HashMap::new();
    let mut running = Vec::new();
    let mut after = None;
    for table in tables {
        if let Hook::PerTable(hook) = &table.hook {
            let (feed, paths) = std::sync::mpsc::channel();
            let (done, next) = std::sync::mpsc::channel();
            let turn = Turn {
                after: after.replace(next),
                done: Some(done),
            };
            running.push((
                table.name.clone(),
                start_hook(Arc::clone(hook), paths.into_iter(), turn),
            ));
            feeds.insert(table.name.clone(), feed);
        }
    }
    (feeds, running)
}

/// A watch batch a foreign key rejected: the events it applied, and the
/// errors that report it.
struct RejectedBatch {
    events: Vec<FileEvent>,
    errors: Vec<RowEvent>,
}

/// The error event for a watch batch that could not be applied: an orphaned
/// row is reported against the table and file that hold it.
fn batch_error(e: DirSqlError) -> RowEvent {
    match e {
        DirSqlError::Core(DbError::ForeignKey(v)) => {
            error_event(Some(&v.table), v.file.as_deref().unwrap_or(""), v.message)
        }
        other => error_event(None, "", other.to_string()),
    }
}

fn error_event(table: Option<&str>, rel_path: &str, error: String) -> RowEvent {
    RowEvent::Error {
        table: table.map(str::to_string),
        file_path: PathBuf::from(rel_path),
        error,
    }
}

/// Apply the watcher's events as they arrive, for as long as the instance
/// lives.
fn run_applier(inner: Weak<DirSqlInner>) {
    loop {
        let Some(db) = inner.upgrade().map(|inner| DirSQL { inner }) else {
            return;
        };
        let watcher = match db.inner.watcher.lock() {
            Ok(guard) => guard.clone(),
            Err(e) => {
                db.deliver(vec![RowEvent::Error {
                    table: None,
                    file_path: db.inner.root.clone(),
                    error: DirSqlError::lock(e).to_string(),
                }]);
                return;
            }
        };
        let interval = db.inner.poll_interval;
        // Held through the wait, the instance would outlive its last handle,
        // and dropping that handle would not close the database.
        drop(db);
        match watcher {
            Some(watcher) => watcher.wait(interval),
            None => thread::sleep(interval),
        }
        let Some(db) = inner.upgrade().map(|inner| DirSQL { inner }) else {
            return;
        };
        db.apply_and_deliver();
    }
}

/// The relative and absolute paths a per-table hook runs over, in file order.
/// The absolute path is the one the scan found. Relative paths are cloned
/// when `keep_rel` (the files are still read afterwards) and moved out
/// otherwise.
/// Each per-file hook's file paired with its hook, then each per-table
/// hook's table with its files, in scan order.
#[allow(clippy::type_complexity)]
fn group_scanned(
    scanned_files: Vec<ScannedFile>,
    on_file_map: &HashMap<String, Hook>,
) -> Result<(
    Vec<(ScannedFile, Arc<OnFileFn>)>,
    Vec<(String, Arc<OnFilesFn>, Vec<ScannedFile>)>,
)> {
    let mut per_file: Vec<(ScannedFile, Arc<OnFileFn>)> = Vec::new();
    let mut batches: Vec<(String, Arc<OnFilesFn>, Vec<ScannedFile>)> = Vec::new();
    for file in scanned_files {
        // Files arrive in walk order, so a table's files mostly follow
        // one another: the last batch is checked before the map.
        if let Some((name, _, files)) = batches.last_mut()
            && *name == file.table_name
        {
            files.push(file);
            continue;
        }
        match on_file_map.get(&file.table_name) {
            Some(Hook::PerFile(hook)) => {
                if !file.trusted {
                    per_file.push((file, Arc::clone(hook)));
                }
            }
            Some(Hook::PerTable(hook)) => {
                match batches
                    .iter_mut()
                    .find(|(name, _, _)| *name == file.table_name)
                {
                    Some((_, _, files)) => files.push(file),
                    None => batches.push((file.table_name.clone(), Arc::clone(hook), vec![file])),
                }
            }
            None => {
                return Err(DirSqlError::Ddl(format!(
                    "missing on-file function for table {}",
                    file.table_name
                )));
            }
        }
    }
    Ok((per_file, batches))
}

/// Insert the rows of every hook started before the scan, in the order the
/// hooks run.
fn stream_early(
    db: &Db,
    early: Vec<(String, Production)>,
    strict_map: &HashMap<String, bool>,
) -> Result<()> {
    for (table_name, production) in early {
        let strict = *strict_map.get(&table_name).unwrap_or(&false);
        if let Err(message) =
            stream_table_rows(db, &table_name, production, strict).map_err(map_db_error)?
        {
            return Err(DirSqlError::TableCommand {
                name: table_name,
                message,
            });
        }
    }
    Ok(())
}

fn declared_position(declared: &[String], table: &str) -> Option<usize> {
    declared.iter().position(|name| name == table)
}

/// Split off the batches of tables whose hooks already ran, keeping only
/// each one's relative paths in scan order.
#[allow(clippy::type_complexity)]
fn split_streamed(
    batches: Vec<(String, Arc<OnFilesFn>, Vec<ScannedFile>)>,
    streamed: &HashSet<String>,
) -> (
    Vec<(String, Arc<OnFilesFn>, Vec<ScannedFile>)>,
    Vec<(String, Vec<String>)>,
) {
    let mut rest = Vec::new();
    let mut done = Vec::new();
    for (table_name, hook, mut files) in batches {
        if streamed.contains(&table_name) {
            done.push((table_name, batch_paths(&mut files, false).0));
        } else {
            rest.push((table_name, hook, files));
        }
    }
    (rest, done)
}

fn batch_paths(files: &mut [ScannedFile], keep_rel: bool) -> (Vec<String>, Vec<PathBuf>) {
    let mut rel = Vec::with_capacity(files.len());
    let mut abs = Vec::with_capacity(files.len());
    for file in files {
        abs.push(std::mem::take(&mut file.abs_path));
        rel.push(if keep_rel {
            file.rel_path.clone()
        } else {
            std::mem::take(&mut file.rel_path)
        });
    }
    (rel, abs)
}

/// Build [`Table`] objects from a parsed config.
///
/// A config-defined table runs its `on-file` command once over every matched
/// file (see [`run_on_files`]): the command receives the absolute paths as
/// trailing arguments and prints one JSON row object per line on stdout,
/// which becomes the table's rows verbatim. The core injects nothing — a DDL
/// column the hook does not emit is NULL, validated against the DDL as usual.
/// `config_dir` is the command's working directory (the config file's parent)
/// and `root` is the resolved index root exposed as the `{root}` placeholder.
/// Runs are unbounded; a config that wants a bound wraps its command in
/// `timeout(1)`.
/// Whether this build declared no tables by any route — neither a config file
/// nor a programmatic table. That is exactly the state in which `files` used to
/// exist implicitly, and so the only state whose missing-`files` error earns the
/// path-table hint. Pure so the whole truth table is unit-testable without I/O.
fn is_configless(config_paths: &[PathBuf], tables: &[Table]) -> bool {
    config_paths.is_empty() && tables.is_empty()
}

/// Fold one config's `[[dirsql.function]]` entries into the accumulated
/// resolved list. Each function's worker runs from its own config's directory
/// (`cfg_dir`), and its per-call timeout falls back to the function
/// mechanism's own 30s default. A name declared by two configs — or twice in
/// one — is a hard error naming both sources, the same rule duplicate tables
/// follow.
fn resolve_functions(
    specs: Vec<config::FunctionSpec>,
    cfg_path: &Path,
    cfg_dir: &Path,
    sources: &mut HashMap<String, PathBuf>,
    out: &mut Vec<ResolvedFunction>,
) -> Result<()> {
    for spec in specs {
        if let Some(prior) = sources.insert(spec.name.clone(), cfg_path.to_path_buf()) {
            return Err(DirSqlError::config(
                config::ConfigError::DuplicateFunction {
                    name: spec.name,
                    first: Source::Path(prior),
                    second: Source::Path(cfg_path.to_path_buf()),
                },
            ));
        }
        out.push(ResolvedFunction {
            name: spec.name,
            args: spec.args,
            command: spec.command,
            deterministic: spec.deterministic,
            timeout: spec.timeout.unwrap_or(functions::DEFAULT_FUNCTION_TIMEOUT),
            batch: spec.batch,
            cwd: cfg_dir.to_path_buf(),
        });
    }
    Ok(())
}

fn build_tables_from_config(cfg: &config::Config, config_dir: &Path) -> Result<Vec<Table>> {
    let mut tables = Vec::with_capacity(cfg.tables.len());
    let home = db::home_dir();

    for table_cfg in &cfg.tables {
        let command = table_cfg.on_file.clone();
        if let Some(message) = on_file::path_placeholder_rejection(&command) {
            return Err(DirSqlError::PathPlaceholder(message));
        }
        let (anchor, glob) =
            path_table::config_anchor(&table_cfg.glob, config_dir, home.as_deref()).ok_or_else(
                || DirSqlError::Core(DbError::PathTable(db::no_home_path_table(&table_cfg.glob))),
            )?;
        let hook_cwd = config_dir.to_path_buf();
        let hook_root = anchor.clone();
        let mut table = Table::per_table_streaming(
            table_cfg.name.clone(),
            table_cfg.ddl.clone(),
            glob,
            move |paths: &mut PathFeed<'_>, sink: &mut RowSink<'_>, end: &mut OutputEnd<'_>| {
                run_on_files(&command, paths, &hook_cwd, &hook_root, sink, end)
            },
        )
        .anchored(anchor)
        .scoped_ignore(cfg.ignore.clone())
        .warning_when_empty();

        if table_cfg.strict == Some(true) {
            table.strict = true;
        }

        tables.push(table);
    }

    Ok(tables)
}

/// Run a table's `on-file` command once over every matched file and hand its
/// rows to `sink` as each invocation's output parses. The absolute paths are
/// appended to the command's argv in scan order; `{root}` is the index root.
/// Any failure is the table's and fails the build.
fn run_on_files(
    command: &str,
    paths: &mut PathFeed<'_>,
    config_dir: &Path,
    root: &Path,
    sink: &mut RowSink<'_>,
    end: &mut OutputEnd<'_>,
) -> std::result::Result<(), BoxError> {
    Ok(on_file::run_streaming(
        command, config_dir, root, paths, sink, end,
    )?)
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
fn generic_watch_error(message: &str) -> notify::Error {
    notify::Error::generic(message)
}

#[cfg(test)]
fn max_files_watch_error() -> notify::Error {
    notify::Error::new(notify::ErrorKind::MaxFilesWatch)
}

#[cfg(test)]
fn path_not_found_error() -> notify::Error {
    notify::Error::path_not_found()
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
        self.query_with(sql, &Params::default()).await
    }

    /// [`DirSQL::query_with`] off the async runtime's worker threads.
    pub async fn query_with(&self, sql: &str, params: &Params) -> Result<Vec<Row>> {
        let db = self.sync()?;
        let sql = sql.to_string();
        let params = params.clone();
        tokio::task::spawn_blocking(move || db.query_with(&sql, &params))
            .await
            .map_err(|e| DirSqlError::Lock(format!("join error: {e}")))?
    }

    pub fn watch(&self) -> Result<WatchStream> {
        self.sync()?.watch()
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
        // `watch`, `config`, `matcher` wrap a typed error to preserve a
        // `source()` chain.
        let io = || std::io::Error::other("x");
        let watch_err = DirSqlError::watch(generic_watch_error("x"));
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
    use std::cell::Cell;
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
        subtrees: StdHashMap<PathBuf, Vec<(PathBuf, String)>>,
        subtree_walks: Mutex<Vec<PathBuf>>,
        gone: Mutex<StdHashSet<PathBuf>>,
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

        /// Register a directory together with what the scanner's walk of it
        /// would return; each file is also stat-able as a regular file.
        fn with_subtree(mut self, dir: impl Into<PathBuf>, found: Vec<(PathBuf, String)>) -> Self {
            let dir = dir.into();
            self.dirs.insert(dir.clone());
            for (path, _) in &found {
                self.stats.insert(path.clone(), fake_stat());
            }
            self.subtrees.insert(dir, found);
            self
        }

        /// Make a registered path vanish, as `rm` would.
        fn remove(&self, path: &Path) {
            self.gone.lock().unwrap().insert(path.to_path_buf());
        }

        fn subtree_walks(&self) -> Vec<PathBuf> {
            self.subtree_walks.lock().unwrap().clone()
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
            self.stats
                .get(path)
                .filter(|_| !self.gone.lock().unwrap().contains(path))
                .cloned()
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "fake: no such file")
                })
        }

        fn is_file(&self, path: &Path) -> std::io::Result<bool> {
            if self.gone.lock().unwrap().contains(path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "fake: no such file",
                ));
            }
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

        fn is_dir(&self, path: &Path) -> std::io::Result<bool> {
            if self.gone.lock().unwrap().contains(path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "fake: no such file",
                ));
            }
            if self.dirs.contains(path) {
                return Ok(true);
            }
            if self.stats.contains_key(path) {
                return Ok(false);
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "fake: no such file",
            ))
        }

        fn scan_subtree(
            &self,
            _root: &Path,
            dir: &Path,
            _matcher: &TableMatcher,
            _on_link: &(dyn Fn(&Path) + Sync),
        ) -> Vec<(PathBuf, String)> {
            self.subtree_walks.lock().unwrap().push(dir.to_path_buf());
            self.subtrees.get(dir).cloned().unwrap_or_default()
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

    /// Records how many times the startup walk ran and hands back a canned
    /// result, so the assertion is about the walk happening, not about any
    /// tree on disk.
    struct RecordingScan {
        calls: Cell<usize>,
        found: Vec<Found>,
    }

    impl RecordingScan {
        fn new(found: Vec<Found>) -> Self {
            Self {
                calls: Cell::new(0),
                found,
            }
        }

        /// Mirrors the real walk closely enough for the seam: it reports a
        /// running file count as it goes, then returns what it found.
        fn scan(
            &self,
            _root: &Path,
            _matcher: &TableMatcher,
            on_file: &mut dyn FnMut(u64),
            _on_match: &mut dyn FnMut(&Path, &str),
        ) -> Vec<Found> {
            self.calls.set(self.calls.get() + 1);
            for (index, _) in self.found.iter().enumerate() {
                on_file(index as u64 + 1);
            }
            self.found.clone()
        }
    }

    fn resolved_over(root: &Path, tables: Vec<Table>, persist: bool) -> ResolvedBuild {
        ResolvedBuild {
            root: root.to_path_buf(),
            tables,
            ignore: Vec::new(),
            extensions: Vec::new(),
            functions: Vec::new(),
            persist,
            persist_path: persist.then(|| root.join("cache.db")),
            poll_interval: DEFAULT_POLL_INTERVAL,
            hint_legacy_files_table: false,
            path_table_parser: None,
            no_ignore: false,
        }
    }

    fn txt_table() -> Table {
        Table::new(
            "items",
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |_| Vec::new(),
        )
    }

    #[test]
    fn prepare_opens_the_database_while_the_scan_is_active() {
        use std::sync::mpsc;

        let dir = TempDir::new().unwrap();
        let (scan_started, wait_for_scan) = mpsc::channel();
        let (db_opened, wait_for_db) = mpsc::channel();
        let scan = |_: &Path,
                    _: &TableMatcher,
                    _: &mut dyn FnMut(u64),
                    _: &mut dyn FnMut(&Path, &str),
                    _: &(dyn Fn(&Path) + Sync)| {
            scan_started.send(()).unwrap();
            assert!(
                wait_for_db.recv_timeout(Duration::from_secs(5)).is_ok(),
                "database creation must finish before the scan completes"
            );
            Vec::new()
        };
        let new_db = move || {
            wait_for_scan.recv_timeout(Duration::from_secs(5)).unwrap();
            let db = Db::new()?;
            db_opened.send(()).unwrap();
            Ok(db)
        };

        let prepared = DirSQL::prepare_resolved_with_db(
            resolved_over(dir.path(), vec![txt_table()], false),
            &scan,
            new_db,
        )
        .unwrap();
        assert!(prepared.scanned_files.into_files().is_empty());
    }

    #[test]
    fn prepare_creates_the_table_while_the_scan_is_active() {
        use std::sync::mpsc;

        let dir = TempDir::new().unwrap();
        let (scan_started, wait_for_scan) = mpsc::channel();
        let (ddl_done, wait_for_ddl) = mpsc::channel();
        let scan = |_: &Path,
                    _: &TableMatcher,
                    _: &mut dyn FnMut(u64),
                    _: &mut dyn FnMut(&Path, &str),
                    _: &(dyn Fn(&Path) + Sync)| {
            scan_started.send(()).unwrap();
            assert!(
                wait_for_ddl.recv_timeout(Duration::from_secs(5)).is_ok(),
                "table DDL must finish before the scan completes"
            );
            Vec::new()
        };
        let ddl = move |db: &Db, tables: &[Table]| {
            wait_for_scan.recv_timeout(Duration::from_secs(5)).unwrap();
            let warnings = run_table_ddl(db, tables)?;
            assert!(catalog_entry(db, "items")?.is_some());
            ddl_done.send(()).unwrap();
            Ok(warnings)
        };

        let prepared = DirSQL::prepare_resolved_with_setup(
            resolved_over(dir.path(), vec![txt_table()], false),
            &scan,
            Db::new,
            ddl,
        )
        .unwrap();
        assert!(prepared.scanned_files.into_files().is_empty());
    }

    #[test]
    fn prepare_defers_ddl_until_after_extensions_load() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = TempDir::new().unwrap();
        let mut resolved = resolved_over(dir.path(), vec![txt_table()], false);
        resolved.extensions.push(Extension {
            path: dir.path().join("missing-extension.so"),
            entrypoint: None,
        });
        let ddl_called = Arc::new(AtomicBool::new(false));
        let called = Arc::clone(&ddl_called);
        let prepared = DirSQL::prepare_resolved_with_setup(
            resolved,
            &|_, _, _, _, _| Vec::new(),
            Db::new,
            move |_, _| {
                called.store(true, Ordering::SeqCst);
                Ok(Vec::new())
            },
        )
        .unwrap();

        assert!(!ddl_called.load(Ordering::SeqCst));
        assert!(!prepared.fresh_db.as_ref().unwrap().1);
        assert!(matches!(
            DirSQL::finish_build(prepared),
            Err(DirSqlError::Extension { .. })
        ));
    }

    #[test]
    fn configure_db_paths_honors_gitignore_unless_told_not_to() {
        for no_ignore in [false, true] {
            let mut db = Db::new().unwrap();
            configure_db_paths(&mut db, Path::new("/r"), &[], false, None, no_ignore);
            assert_eq!(db.path_table_gitignore, !no_ignore);
        }
    }

    #[test]
    fn run_table_ddl_creates_a_missing_table_and_keeps_a_present_one() {
        let db = Db::new().unwrap();
        let tables = [txt_table()];
        assert!(run_table_ddl(&db, &tables).unwrap().is_empty());
        assert!(catalog_entry(&db, "items").unwrap().is_some());
        assert!(run_table_ddl(&db, &tables).unwrap().is_empty());
    }

    #[test]
    fn run_table_ddl_names_only_the_tables_its_batch_created() {
        let db = Db::new().unwrap();
        db.conn()
            .execute_batch("CREATE TABLE earlier (y TEXT)")
            .unwrap();
        let tables = [Table::new(
            "items",
            "CREATE TABLE made (x TEXT)",
            "*.txt",
            |_| Vec::new(),
        )];
        let err = run_table_ddl(&db, &tables).unwrap_err();
        assert!(
            matches!(&err, DirSqlError::TableNotCreated { created, .. } if created == &["made"]),
            "{err:?}"
        );
    }

    #[test]
    fn prepare_defers_ddl_until_after_functions_register() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = TempDir::new().unwrap();
        let mut resolved = resolved_over(dir.path(), vec![txt_table()], false);
        resolved.functions.push(ResolvedFunction {
            name: "custom_value".into(),
            args: vec![0],
            command: "true".into(),
            deterministic: true,
            timeout: Duration::from_secs(1),
            batch: None,
            cwd: dir.path().to_path_buf(),
        });
        let ddl_called = Arc::new(AtomicBool::new(false));
        let called = Arc::clone(&ddl_called);
        let prepared = DirSQL::prepare_resolved_with_setup(
            resolved,
            &|_, _, _, _, _| Vec::new(),
            Db::new,
            move |_, _| {
                called.store(true, Ordering::SeqCst);
                Ok(Vec::new())
            },
        )
        .unwrap();

        assert!(!ddl_called.load(Ordering::SeqCst));
        assert!(!prepared.fresh_db.as_ref().unwrap().1);
        assert!(DirSQL::finish_build(prepared).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_parent_and_child_landing_mid_walk_both_reach_the_build() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (parents, children) = (root.join("parents"), root.join("children"));
        std::fs::create_dir(&parents).unwrap();
        std::fs::create_dir(&children).unwrap();
        let tables = vec![
            Table::new(
                "parent",
                "CREATE TABLE parent (id INTEGER PRIMARY KEY)",
                "*.p",
                |_| vec![Row::from_iter([("id".to_string(), Value::Integer(1))])],
            )
            .anchored(&parents),
            Table::new(
                "child",
                "CREATE TABLE child (pid INTEGER REFERENCES parent(id))",
                "*.c",
                |_| vec![Row::from_iter([("pid".to_string(), Value::Integer(1))])],
            )
            .anchored(&children),
        ];
        let scan = |anchor: &Path,
                    matcher: &TableMatcher,
                    on_file: &mut dyn FnMut(u64),
                    on_match: &mut dyn FnMut(&Path, &str),
                    on_link: &(dyn Fn(&Path) + Sync)| {
            if anchor == children {
                std::fs::write(parents.join("one.p"), "").unwrap();
                std::fs::write(children.join("one.c"), "").unwrap();
            }
            scan_directory_reporting(anchor, matcher, on_file, on_match, on_link)
        };

        let prepared =
            DirSQL::prepare_resolved_with(resolved_over(&root, tables, false), &scan).unwrap();
        let db = DirSQL::finish_build(prepared).unwrap();

        assert_eq!(db.query("SELECT id FROM parent").unwrap().len(), 1);
        assert_eq!(db.query("SELECT pid FROM child").unwrap().len(), 1);
    }

    /// Configless startup declares zero tables, so nothing the walk could
    /// visit can match: the walk must not run at all.
    #[test]
    fn prepare_does_not_walk_the_tree_when_no_tables_are_declared() {
        let dir = TempDir::new().unwrap();
        let scan = RecordingScan::new(vec![Found {
            path: dir.path().join("a.txt"),
            rel_path: "a.txt".into(),
            table_name: "items".into(),
        }]);

        let prepared = DirSQL::prepare_resolved_with(
            resolved_over(dir.path(), Vec::new(), false),
            &|root, matcher, on_file, on_match, _| scan.scan(root, matcher, on_file, on_match),
        )
        .unwrap();

        assert_eq!(
            scan.calls.get(),
            0,
            "zero tables can match nothing, so the startup walk is pure waste"
        );
        assert!(prepared.scanned_files.into_files().is_empty());
    }

    /// The skip is narrow: one declared table still walks, and prepare still
    /// carries what the walk found.
    #[test]
    fn prepare_walks_the_tree_when_a_table_is_declared() {
        let dir = TempDir::new().unwrap();
        let scan = RecordingScan::new(vec![Found {
            path: dir.path().join("a.txt"),
            rel_path: "a.txt".into(),
            table_name: "items".into(),
        }]);

        let prepared = DirSQL::prepare_resolved_with(
            resolved_over(dir.path(), vec![txt_table()], false),
            &|root, matcher, on_file, on_match, _| scan.scan(root, matcher, on_file, on_match),
        )
        .unwrap();

        assert_eq!(scan.calls.get(), 1);
        let found: Vec<String> = prepared
            .scanned_files
            .into_files()
            .into_iter()
            .map(|f| f.rel_path)
            .collect();
        assert_eq!(found, vec!["a.txt"]);
    }

    /// A per-table hook is running over a path before the walk has finished.
    #[test]
    fn prepare_starts_a_per_table_hook_while_the_walk_is_still_running() {
        let dir = TempDir::new().unwrap();
        let (got, got_rx) = std::sync::mpsc::channel::<PathBuf>();
        let got = Mutex::new(got);
        let items = Table::per_table_streaming(
            "items",
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            move |paths: &mut PathFeed<'_>, _: &mut RowSink<'_>, _: &mut OutputEnd<'_>| {
                for path in paths {
                    let _ = got.lock().unwrap().send(path);
                }
                Ok(())
            },
        );
        let resolved = resolved_over(dir.path(), vec![items], false);
        let hook_saw_it_mid_walk = Cell::new(false);
        let found = dir.path().join("a.txt");
        DirSQL::prepare_resolved_with(resolved, &|_, _, _, on_match, _| {
            on_match(&found, "items");
            hook_saw_it_mid_walk.set(
                got_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .is_ok(),
            );
            vec![Found {
                path: found.clone(),
                rel_path: "a.txt".into(),
                table_name: "items".into(),
            }]
        })
        .unwrap();
        assert!(hook_saw_it_mid_walk.get());
    }

    /// The startup walk honors `.gitignore` unless the build asked not to.
    #[test]
    fn prepare_hands_the_walk_a_matcher_that_honors_gitignore_unless_no_ignore() {
        let dir = TempDir::new().unwrap();
        for no_ignore in [false, true] {
            let honored = Cell::new(None);
            let mut resolved = resolved_over(dir.path(), vec![txt_table()], false);
            resolved.no_ignore = no_ignore;
            DirSQL::prepare_resolved_with(resolved, &|_, matcher, _, _, _| {
                honored.set(Some(matcher.respects_gitignore()));
                Vec::new()
            })
            .unwrap();
            assert_eq!(honored.get(), Some(!no_ignore));
        }
    }

    /// Skipping the walk skips only the walk: with persist on, the cache is
    /// still opened and reconciled against the (empty) scan.
    #[test]
    fn prepare_still_opens_the_persist_cache_when_no_tables_are_declared() {
        let dir = TempDir::new().unwrap();
        let scan = RecordingScan::new(Vec::new());

        let prepared = DirSQL::prepare_resolved_with(
            resolved_over(dir.path(), Vec::new(), true),
            &|root, matcher, on_file, on_match, _| scan.scan(root, matcher, on_file, on_match),
        )
        .unwrap();

        assert_eq!(scan.calls.get(), 0);
        let persist = prepared.persist.expect("persist context");
        assert!(persist.deleted.is_empty());
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
    fn builder_defaults_to_respecting_gitignore() {
        let resolved = DirSQL::builder().root("/idx").resolve().unwrap();
        assert!(!resolved.no_ignore);
    }

    #[test]
    fn resolve_makes_a_relative_root_absolute() {
        let resolved = DirSQL::builder().root("tree").resolve().unwrap();
        assert!(resolved.root.is_absolute(), "got {:?}", resolved.root);
        assert!(resolved.root.ends_with("tree"), "got {:?}", resolved.root);
    }

    #[test]
    fn builder_no_ignore_flows_into_the_resolved_build() {
        let resolved = DirSQL::builder()
            .root("/idx")
            .no_ignore(true)
            .resolve()
            .unwrap();
        assert!(resolved.no_ignore);
    }

    /// A `ScannedFile` whose table has no registered on_file function must
    /// error rather than be silently skipped.
    #[test]
    fn finish_build_errors_on_ghost_scanned_file() {
        let dir = TempDir::new().unwrap();
        let prepared = PreparedBuild {
            ignore: Vec::new(),
            root: dir.path().to_path_buf(),
            tables: Vec::new(),
            extensions: Vec::new(),
            functions: Vec::new(),
            groups: Vec::new(),
            anchors: TableAnchors::at_root(dir.path()),
            scanned_files: Scanned::Files(vec![ScannedFile {
                abs_path: PathBuf::new(),
                rel_path: "ghost.txt".into(),
                table_name: "ghost".into(),
                stat: None,
                trusted: false,
            }]),
            fresh_db: Some((Db::new().unwrap(), false, Vec::new())),
            early: Vec::new(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            persist: None,
            hint_legacy_files_table: false,
            path_table_parser: None,
            no_ignore: false,
            links: Vec::new(),
            watcher: None,
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
                "items",
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

    fn subtree_db(fake: FakeFs, root: &Path, ignore: Vec<&str>) -> DirSQL {
        DirSQL::with_ignore_and_fs(
            root,
            vec![Table::new(
                "items",
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |path| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text(path.to_string()),
                    )])]
                },
            )],
            ignore,
            Arc::new(fake),
        )
        .unwrap()
    }

    /// A created directory is indexed by walking it: every file the scanner
    /// reports beneath it becomes rows, as if each had its own create event.
    #[test]
    fn process_file_event_indexes_the_files_beneath_a_created_directory() {
        let dir = TempDir::new().unwrap();
        let moved = dir.path().join("moved");
        let a = moved.join("a.txt");
        let deep = moved.join("one").join("deep.txt");
        let fake = FakeFs::default().with_subtree(
            moved.clone(),
            vec![(a.clone(), "items".into()), (deep.clone(), "items".into())],
        );
        let db = subtree_db(fake, dir.path(), Vec::new());

        let events = db.process_file_event(FileEvent::Created(moved));

        assert_eq!(events.len(), 2, "one insert per file found: {events:?}");
        assert!(
            events.iter().all(|e| matches!(e, RowEvent::Insert { .. })),
            "got: {events:?}"
        );
        let paths: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                RowEvent::Insert { file_path, .. } => Some(file_path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            vec!["moved/a.txt".to_string(), "moved/one/deep.txt".to_string()],
            "file paths are root-relative"
        );
    }

    fn failing_subtree_db(fake: FakeFs, root: &Path) -> DirSQL {
        DirSQL::with_ignore_and_fs(
            root,
            vec![Table::try_new(
                "items",
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| Err("half written".into()),
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap()
    }

    /// A file found inside a just-created directory may still be mid-write,
    /// so failing to parse it is no error: its close-write reads it again.
    #[test]
    fn a_created_directory_skips_a_file_that_fails_to_parse() {
        let dir = TempDir::new().unwrap();
        let moved = dir.path().join("moved");
        let fake = FakeFs::default()
            .with_subtree(moved.clone(), vec![(moved.join("a.txt"), "items".into())]);
        let db = failing_subtree_db(fake, dir.path());

        let events = db.process_file_event(FileEvent::Created(moved));

        assert!(events.is_empty(), "got: {events:?}");
    }

    /// A file that is no longer being written does report its parse failure.
    #[test]
    fn a_closed_file_that_fails_to_parse_reports_the_error() {
        let dir = TempDir::new().unwrap();
        let moved = dir.path().join("moved");
        let a = moved.join("a.txt");
        let fake = FakeFs::default().with_subtree(moved, vec![(a.clone(), "items".into())]);
        let db = failing_subtree_db(fake, dir.path());

        let events = db.process_file_event(FileEvent::Modified(a));

        assert!(
            matches!(&events[..], [RowEvent::Error { error, .. }] if error.contains("half written")),
            "got: {events:?}"
        );
    }

    /// The walk is matched against the root the event was stripped with, so
    /// the rows it writes carry the same relative paths a create event would.
    #[test]
    fn process_file_event_walks_a_created_directory_under_the_watch_root() {
        let root = PathBuf::from("/ws");
        let moved = root.join("moved");
        let fake = FakeFs::default()
            .with_canonical_root(&root, "/ws")
            .with_subtree(moved.clone(), vec![(moved.join("a.txt"), "items".into())]);
        let db = subtree_db(fake, &root, Vec::new());

        let events = db.process_file_event(FileEvent::Created(moved.clone()));

        assert!(
            matches!(&events[..], [RowEvent::Insert { file_path, .. }] if file_path == "moved/a.txt"),
            "got: {events:?}"
        );
    }

    /// Only a *created* directory is walked. A metadata change on one says
    /// nothing about its files, and walking on every such event would re-run
    /// `on_file` across the whole subtree for a `chmod`.
    #[test]
    fn process_file_event_does_not_walk_a_modified_directory() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        let fake =
            FakeFs::default().with_subtree(sub.clone(), vec![(sub.join("a.txt"), "items".into())]);
        let fake = Arc::new(fake);
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::new(
                "items",
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| vec![],
            )],
            Vec::<String>::new(),
            fake.clone(),
        )
        .unwrap();

        let events = db.process_file_event(FileEvent::Modified(sub.clone()));

        assert!(events.is_empty(), "got: {events:?}");
        assert!(
            fake.subtree_walks().is_empty(),
            "a modified directory must not be walked; walked: {:?}",
            fake.subtree_walks()
        );
    }

    /// An ignored directory is never walked, matching the initial scan.
    #[test]
    fn process_file_event_does_not_walk_an_ignored_created_directory() {
        let dir = TempDir::new().unwrap();
        let skip = dir.path().join("skip");
        let fake = Arc::new(
            FakeFs::default()
                .with_subtree(skip.clone(), vec![(skip.join("a.txt"), "items".into())]),
        );
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::new(
                "items",
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |_| vec![],
            )],
            vec!["skip"],
            fake.clone(),
        )
        .unwrap();

        let events = db.process_file_event(FileEvent::Created(skip));

        assert!(events.is_empty(), "got: {events:?}");
        assert!(
            fake.subtree_walks().is_empty(),
            "ignored directory was walked"
        );
    }

    /// A one-table db whose fake fs stats every file in `files`, each already
    /// upserted so the db holds a row per file. Queries read the index as
    /// applied, since the fake walk lists no files.
    fn populated_db(root: &Path, glob: &str, files: &[&str]) -> DirSQL {
        let mut fake = FakeFs::default();
        for f in files {
            fake.stats.insert(root.join(f), fake_stat());
        }
        let db = DirSQL::with_ignore_and_fs(
            root,
            vec![Table::new(
                "items",
                "CREATE TABLE items (name TEXT)",
                glob,
                |path| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text(path.to_string()),
                    )])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();
        for f in files {
            let events = db.handle_upsert("items", &root.join(f), f);
            assert!(
                matches!(&events[..], [RowEvent::Insert { .. }]),
                "got: {events:?}"
            );
        }
        db
    }

    fn row_names(db: &DirSQL) -> Vec<String> {
        let mut names: Vec<String> = db
            .query("SELECT name FROM items")
            .unwrap()
            .into_iter()
            .filter_map(|r| match r.get("name") {
                Some(Value::Text(n)) => Some(n.clone()),
                _ => None,
            })
            .collect();
        names.sort();
        names
    }

    fn await_row_names(db: &DirSQL, expected: &[&str]) {
        for _ in 0..1000 {
            if row_names(db) == expected {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(row_names(db), expected);
    }

    fn deleted_paths(events: &[RowEvent]) -> Vec<String> {
        assert!(
            events.iter().all(|e| matches!(e, RowEvent::Delete { .. })),
            "expected only Delete events: {events:?}"
        );
        let mut paths: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                RowEvent::Delete { file_path, .. } => Some(file_path.clone()),
                _ => None,
            })
            .collect();
        paths.sort();
        paths
    }

    /// A deleted directory matches no file glob, yet every row recorded
    /// beneath it goes, with one Delete per file. A sibling whose name merely
    /// starts with the directory's (`moved2/`) is untouched.
    #[test]
    fn process_file_event_deleted_directory_removes_the_rows_beneath_it() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let files = ["moved/a.txt", "moved/one/deep.txt", "moved2/x.txt"];
        let db = populated_db(&root, "**/*.txt", &files);

        let events = db.process_file_event(FileEvent::Deleted(root.join("moved")));

        assert_eq!(
            deleted_paths(&events),
            vec!["moved/a.txt".to_string(), "moved/one/deep.txt".to_string()]
        );
        assert_eq!(
            row_names(&db),
            vec![root.join("moved2/x.txt").to_string_lossy().to_string()]
        );
    }

    /// With a glob that also matches the directory itself (`**/*`), the
    /// exact-path delete finds no row and the subtree delete still runs.
    #[test]
    fn process_file_event_deleted_directory_matching_a_glob_removes_the_rows_beneath_it() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let db = populated_db(&root, "**/*", &["moved/a.txt", "keep.txt"]);

        let events = db.process_file_event(FileEvent::Deleted(root.join("moved")));

        assert_eq!(deleted_paths(&events), vec!["moved/a.txt".to_string()]);
        assert_eq!(
            row_names(&db),
            vec![root.join("keep.txt").to_string_lossy().to_string()]
        );
    }

    /// Only a *deleted* directory empties its subtree; a metadata change on
    /// one says nothing about its files.
    #[test]
    fn process_file_event_keeps_the_rows_beneath_a_modified_directory() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let db = populated_db(&root, "**/*.txt", &["moved/a.txt"]);

        let events = db.process_file_event(FileEvent::Modified(root.join("moved")));

        assert!(events.is_empty(), "got: {events:?}");
        assert_eq!(
            row_names(&db),
            vec![root.join("moved/a.txt").to_string_lossy().to_string()]
        );
    }

    #[test]
    fn delete_subtree_marks_only_this_groups_tables_holding_files_beneath_it() {
        let (_dir, db, _abs, _rel) = upsert_fixture();
        let tables = vec![
            anchored_table("held", "*.txt", "/one"),
            anchored_table("bare", "*.txt", "/one"),
        ];
        let (groups, _, _) = compile_groups(&tables, &[], Path::new("/idx")).unwrap();
        let group = &groups[0];
        {
            let mut files = db.inner.batch_files.lock().unwrap();
            files.insert("held".into(), vec!["moved/x".into()]);
            files.insert("bare".into(), vec!["elsewhere/y".into()]);
            files.insert("foreign".into(), vec!["moved/z".into()]);
        }
        let mut pending = PendingRefresh::default();

        db.delete_subtree(group, "moved", &mut pending);

        assert_eq!(pending.0, vec![("held".to_string(), "moved".to_string())]);
    }

    #[test]
    fn delete_subtree_forgets_the_failures_of_this_groups_tables_beneath_it() {
        let (_dir, db, _abs, _rel) = upsert_fixture();
        let tables = vec![anchored_table("held", "*.txt", "/one")];
        let (groups, _, _) = compile_groups(&tables, &[], Path::new("/idx")).unwrap();
        db.record_read("held", "moved/x", Some("broken"));
        db.record_read("foreign", "moved/z", Some("broken"));

        db.delete_subtree(&groups[0], "moved", &mut PendingRefresh::default());

        assert_eq!(
            failing_paths(&db),
            vec![("foreign".to_string(), "moved/z".to_string())]
        );
    }

    fn found_at(root: &Path, path: &Path, table: &str) -> Found {
        Found {
            path: path.to_path_buf(),
            rel_path: scanner::to_slash(path.strip_prefix(root).unwrap()),
            table_name: table.to_string(),
        }
    }

    #[test]
    fn scan_groups_counts_across_groups_and_collects_every_match() {
        let tables = vec![
            anchored_table("a", "*.txt", "/one"),
            anchored_table("b", "*.txt", "/two"),
        ];
        let (groups, _, _) = compile_groups(&tables, &[], Path::new("/idx")).unwrap();
        let scan = |anchor: &Path,
                    _: &TableMatcher,
                    report: &mut dyn FnMut(u64),
                    _: &mut dyn FnMut(&Path, &str),
                    _: &(dyn Fn(&Path) + Sync)| {
            report(3);
            vec![found_at(anchor, &anchor.join("f.txt"), "t")]
        };

        let (scanned, seen) = scan_groups(
            &groups,
            &scan,
            &mut Progress::scanning(),
            &mut |_, _| {},
            &|_| {},
        );

        assert_eq!(seen, 6);
        assert_eq!(scanned.len(), 2);
    }

    #[test]
    fn delete_subtree_surfaces_db_poison() {
        let (_dir, db, _abs, _rel) = upsert_fixture();
        poison(&db.inner.db);
        let events =
            db.delete_subtree(&db.inner.groups[0], "moved", &mut PendingRefresh::default());
        assert_single_lock_error(&events);
    }

    #[test]
    fn delete_subtree_surfaces_db_failure() {
        let (_dir, db, _abs, _rel) = upsert_fixture();
        db.inner
            .db
            .lock()
            .unwrap()
            .conn()
            .execute("DROP TABLE _dirsql_internal_rows", [])
            .unwrap();
        let events =
            db.delete_subtree(&db.inner.groups[0], "moved", &mut PendingRefresh::default());
        assert_eq!(events.len(), 1, "expected one error event: {events:?}");
        assert!(
            matches!(&events[0], RowEvent::Error { table: None, file_path, error }
                if file_path == Path::new("moved") && error.contains("no such table")),
            "got: {events:?}"
        );
    }

    /// Building with a **relative** root canonicalizes the watch anchor to an
    /// absolute path while leaving `root` exactly as the caller supplied it,
    /// so `notify` never sees `.`.
    #[test]
    fn relative_root_canonicalizes_watch_anchor_only() {
        let canonical = if cfg!(windows) {
            r"C:\ws\canonical"
        } else {
            "/ws/canonical"
        };
        let fake = FakeFs::default().with_canonical_root(".", canonical);
        let db = DirSQL::with_ignore_and_fs(
            ".",
            vec![Table::new(
                "t",
                "CREATE TABLE t (x TEXT)",
                "*.txt",
                |_| vec![],
            )],
            Vec::<String>::new(),
            Arc::new(fake),
        )
        .unwrap();

        assert_eq!(db.inner.root, PathBuf::from("."));
        assert!(
            db.inner.groups[0].watch_anchor.is_absolute(),
            "watch_anchor must be absolute, got {:?}",
            db.inner.groups[0].watch_anchor
        );
        assert_eq!(db.inner.groups[0].watch_anchor, PathBuf::from(canonical));
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
                "items",
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
                "items",
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
        Arc::get_mut(&mut db.inner).unwrap().groups[0].watch_anchor = root.join("does-not-prefix");

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
                "items",
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
            meta_current: false,
            needs_sweep: false,
        };
        let scanned = vec![found_at(dir.path(), &abs, "t")];
        let (files, deleted) = reconcile_scan(scanned, &ctx, &fake).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].trusted);
        assert_eq!(files[0].rel_path, "a.txt");
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
            meta_current: false,
            needs_sweep: false,
        };
        let scanned = vec![found_at(dir.path(), &abs, "t")];
        let (files, _deleted) = reconcile_scan(scanned, &ctx, &fake).unwrap();
        assert_eq!(files.len(), 1);
        assert!(!files[0].trusted);
    }

    #[test]
    fn reconcile_scan_errors_when_file_vanished() {
        let dir = TempDir::new().unwrap();
        let ctx = PersistContext {
            db: Db::new().unwrap(),
            cached: HashMap::new(),
            expected_meta: HashMap::new(),
            meta_current: false,
            needs_sweep: false,
        };
        let missing = dir.path().join("ghost.txt");
        let scanned = vec![found_at(dir.path(), &missing, "t")];
        let fake = FakeFs::default();
        assert!(reconcile_scan(scanned, &ctx, &fake).is_err());
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
        assert!(
            fs.is_dir(dir.path()).unwrap(),
            "a directory must report is_dir=true"
        );
        assert!(
            fs.is_dir(&missing).is_err(),
            "is_dir of a missing path must error"
        );
        let matcher = TableMatcher::new(&[("**/*", "files")], &[]).unwrap();
        assert!(
            fs.scan_subtree(dir.path(), dir.path(), &matcher, &|_| {})
                .is_empty(),
            "an empty directory walks to nothing"
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
        let db = DirSQL::with_ignore(
            dir.path(),
            vec![Table::new(
                "t",
                "CREATE TABLE t (x TEXT)",
                "*.none",
                |_| vec![],
            )],
            Vec::<String>::new(),
        )
        .unwrap();
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
                "items",
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
    fn query_with_binds_the_params() {
        let (_dir, db) = simple_db();
        let rows = db
            .query_with(
                "SELECT :n AS n",
                &Params::Named(HashMap::from([("n".into(), Value::Integer(4))])),
            )
            .unwrap();
        assert_eq!(rows, [HashMap::from([("n".into(), Value::Integer(4))])]);
    }

    #[test]
    fn query_ordered_with_binds_the_params_in_projection_order() {
        let (_dir, db) = simple_db();
        let result = db
            .query_ordered_with(
                "SELECT ? AS b, ? AS a",
                &Params::Positional(vec![Value::Integer(1), Value::Integer(2)]),
            )
            .unwrap();
        assert_eq!(result.columns, ["b", "a"]);
        assert_eq!(result.rows[0]["a"], Value::Integer(2));
    }

    #[test]
    fn query_ordered_binds_no_params() {
        let (_dir, db) = simple_db();
        let err = db.query_ordered("SELECT ?").unwrap_err();
        assert!(
            matches!(err, DirSqlError::Core(DbError::Params(_))),
            "got: {err:?}"
        );
    }

    #[test]
    fn query_each_with_binds_the_params() {
        let (_dir, db) = simple_db();
        let rows = db
            .query_each_with(
                "SELECT ? AS n",
                &Params::Positional(vec![Value::Text("x".into())]),
                |_| Vec::new(),
                |rows, cells| rows.push(cells.to_vec()),
            )
            .unwrap();
        assert_eq!(rows, [[Value::Text("x".into())]]);
    }

    #[test]
    fn query_each_binds_no_params() {
        let (_dir, db) = simple_db();
        let err = db.query_each("SELECT ?", |_| (), |_, _| {}).unwrap_err();
        assert!(
            matches!(err, DirSqlError::Core(DbError::Params(_))),
            "got: {err:?}"
        );
    }

    #[test]
    fn query_each_with_surfaces_lock_poison() {
        let (_dir, db) = simple_db();
        poison(&db.inner.db);
        let err = db
            .query_each_with("SELECT 1", &Params::default(), |_| (), |_, _| {})
            .unwrap_err();
        assert!(err.to_string().starts_with("failed to lock"), "got: {err}");
    }

    #[test]
    fn attach_watcher_surfaces_lock_poison() {
        let (_dir, db) = simple_db();
        poison(&db.inner.watcher);
        let err = db.attach_watcher().unwrap_err();
        assert!(err.to_string().starts_with("failed to lock"), "got: {err}");
    }

    #[test]
    fn poll_events_surfaces_lock_poison() {
        let (_dir, db) = simple_db();
        poison(&db.inner.polled);
        let err = db.poll_events(Duration::from_millis(0)).unwrap_err();
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
            vec![Table::new(
                "files",
                "CREATE TABLE files (name TEXT)",
                "**/*",
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
                "items",
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
    fn run_applier_reports_a_failure_to_the_stream_and_stops() {
        let (_dir, db) = simple_db();
        let mut rx = db.watch().unwrap();
        poison(&db.inner.watcher);

        run_applier(Arc::downgrade(&db.inner));

        let event = rx.try_recv().expect("expected an error event");
        let dbg = format!("{event:?}");
        assert!(dbg.contains("Error"), "expected an Error event: {dbg}");
        assert!(dbg.contains("failed to lock"), "expected lock text: {dbg}");
    }

    #[test]
    fn run_applier_ends_once_the_instance_is_dropped() {
        let (_dir, db) = simple_db();
        let inner = Arc::downgrade(&db.inner);
        drop(db);
        run_applier(inner);
    }

    #[test]
    fn deliver_without_a_stream_drops_the_events() {
        let (_dir, db) = simple_db();
        db.deliver(vec![error_event(None, "a", "x".into())]);
        assert!(db.inner.stream.lock().unwrap().is_none());
    }

    #[test]
    fn deliver_hands_no_empty_batch_to_poll_events() {
        let (_dir, db) = simple_db();
        db.attach_poll().unwrap();
        db.deliver(Vec::new());
        let polled = db.inner.polled.lock().unwrap();
        assert!(polled.as_ref().unwrap().try_recv().is_err());
    }

    #[test]
    fn deliver_detaches_a_stream_whose_receiver_is_gone() {
        let (_dir, db) = simple_db();
        drop(db.watch().unwrap());
        db.deliver(vec![error_event(None, "a", "x".into())]);
        assert!(db.inner.stream.lock().unwrap().is_none());
    }

    #[test]
    fn is_configless_only_when_config_and_tables_are_both_empty() {
        // Pure truth table (no I/O): the missing-`files` hint is armed only
        // when neither a config path nor a programmatic table was supplied.
        let table = Table::new("x", "CREATE TABLE x (a TEXT)", "*", |_| vec![Row::new()]);
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
            .path_table_parser("parse.py")
            .resolve()
            .unwrap();
        assert_eq!(resolved.path_table_parser.as_deref(), Some("parse.py"));
    }

    #[test]
    fn path_table_parser_with_a_path_placeholder_is_rejected_at_resolve() {
        let err = match DirSQL::builder()
            .root("/tmp/x")
            .path_table_parser("parse.py {path}")
            .resolve()
        {
            Err(err) => err,
            Ok(_) => panic!("`{{path}}` must be rejected"),
        };
        assert!(
            matches!(&err, DirSqlError::PathPlaceholder(m) if m.contains("parse.py {path}")),
            "got: {err}"
        );
    }

    #[test]
    fn resolve_with_a_programmatic_table_disarms_the_hint() {
        let with_table = DirSQL::builder()
            .root("/tmp/x")
            .table(Table::new("t", "CREATE TABLE t (a TEXT)", "*.t", |_| {
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
        let explicit = if cfg!(windows) {
            r"C:\some\explicit\root"
        } else {
            "/some/explicit/root"
        };
        let resolved = DirSQL::builder().root(explicit).resolve().unwrap();
        assert_eq!(resolved.root, PathBuf::from(explicit));
    }

    #[test]
    fn duplicate_table_names_are_rejected() {
        let dir = TempDir::new().unwrap();
        let err = match DirSQL::new(
            dir.path(),
            vec![
                Table::new("t", "CREATE TABLE t (a TEXT)", "*.a", |_| vec![]),
                Table::new("t", "CREATE TABLE t (b TEXT)", "*.b", |_| vec![]),
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
    fn poll_events_returns_empty_without_activity() {
        let (_dir, db) = simple_db();
        assert!(db.poll_events(Duration::from_millis(0)).unwrap().is_empty());
    }

    #[test]
    fn watch_locks_out_poll_events() {
        let (_dir, db) = simple_db();
        let _stream = db.watch().unwrap();
        let e1 = db.poll_events(Duration::from_millis(0)).unwrap_err();
        assert!(e1.to_string().contains("watch() is active"), "got: {e1}");
    }

    #[test]
    fn queued_file_events_hands_over_what_the_watcher_holds_without_waiting() {
        let dir = TempDir::new().unwrap();
        let table = Table::new("t", "CREATE TABLE t (x TEXT)", "*.none", |_| vec![]);
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![table],
            Vec::<String>::new(),
            Arc::new(RealFs),
        )
        .unwrap();
        std::fs::write(dir.path().join("a.none"), "x").unwrap();
        let mut held = Vec::new();
        while !held.iter().any(|e| matches!(e, FileEvent::Created(_))) {
            held.extend(db.queued_file_events());
        }
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
    fn watch_without_named_tables_names_the_cause() {
        let db = DirSQL::new(TempDir::new().unwrap().path(), vec![]).unwrap();
        let err = db.watch().unwrap_err().to_string();
        assert!(
            err.contains("path-tables emit no watch events"),
            "got: {err}"
        );
        assert!(err.contains("define a table"), "got: {err}");
    }

    #[test]
    fn poll_events_without_named_tables_names_the_cause() {
        let db = DirSQL::new(TempDir::new().unwrap().path(), vec![]).unwrap();
        let err = db.poll_events(Duration::ZERO).unwrap_err().to_string();
        assert!(
            err.contains("path-tables emit no watch events"),
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
    fn handle_upsert_updates_a_keyed_row_in_place() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        std::fs::write(&abs, "Alpha").unwrap();
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::new(
                "items",
                "CREATE TABLE items (id INTEGER PRIMARY KEY, slug TEXT UNIQUE, name TEXT)",
                "*.txt",
                |path| {
                    vec![Row::from_iter([
                        ("slug".to_string(), Value::Text("a".into())),
                        (
                            "name".to_string(),
                            Value::Text(std::fs::read_to_string(path).unwrap()),
                        ),
                    ])]
                },
            )],
            Vec::<String>::new(),
            Arc::new(RealFs),
        )
        .unwrap();
        std::fs::write(&abs, "Renamed").unwrap();

        let events = db.handle_upsert("items", &abs, "a.txt");

        match events.as_slice() {
            [RowEvent::Update { new_row, .. }] => {
                assert_eq!(new_row["id"], Value::Integer(1));
                assert_eq!(new_row["name"], Value::Text("Renamed".into()));
            }
            other => panic!("expected one update, got: {other:?}"),
        }
    }

    fn experiment_json(folder: &str, name: &str) -> infer::JsonRow {
        infer::JsonRow(vec![
            (Arc::from("folder_name"), serde_json::json!(folder)),
            (Arc::from("name"), serde_json::json!(name)),
        ])
    }

    fn produced(
        chunks: Vec<Vec<infer::JsonRow>>,
        outcome: std::result::Result<(), String>,
    ) -> Production {
        let hook: Arc<OnFilesFn> = Arc::new(move |_paths, sink, _end| {
            for chunk in chunks.clone() {
                sink(chunk);
            }
            outcome.clone().map_err(BoxError::from)
        });
        start_hook(
            hook,
            std::iter::once(PathBuf::from("rows.json")),
            Turn::default(),
        )
    }

    fn stored_experiments() -> Db {
        let db = Db::new().unwrap();
        db.create_table(
            "experiments",
            "CREATE TABLE experiments (id INTEGER PRIMARY KEY, folder_name TEXT UNIQUE, name TEXT)",
        )
        .unwrap();
        let rows = db
            .shape_rows(
                "experiments",
                vec![experiment_json("a", "Alpha"), experiment_json("b", "Beta")],
                false,
            )
            .unwrap();
        db.insert_rows("experiments", &rows, BATCH_OWNER).unwrap();
        db
    }

    fn experiment_names(db: &Db) -> Vec<(i64, String)> {
        db.conn()
            .prepare("SELECT id, name FROM experiments ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn stream_table_rows_refills_stored_keyed_rows_in_place() {
        let db = stored_experiments();
        let production = produced(
            vec![
                vec![experiment_json("0", "Zero")],
                vec![
                    experiment_json("a", "Alpha"),
                    experiment_json("b", "Renamed"),
                ],
            ],
            Ok(()),
        );

        let outcome = stream_table_rows(&db, "experiments", production, false).unwrap();

        assert_eq!(outcome, Ok(()));
        assert_eq!(
            experiment_names(&db),
            vec![
                (1, "Alpha".into()),
                (2, "Renamed".into()),
                (3, "Zero".into())
            ]
        );
    }

    #[test]
    fn stream_table_rows_refill_writes_nothing_when_the_hook_fails() {
        let db = stored_experiments();
        let production = produced(
            vec![vec![experiment_json("a", "Changed")]],
            Err("boom".into()),
        );

        let outcome = stream_table_rows(&db, "experiments", production, false).unwrap();

        assert_eq!(outcome, Err("boom".into()));
        assert_eq!(
            experiment_names(&db),
            vec![(1, "Alpha".into()), (2, "Beta".into())]
        );
    }

    #[test]
    fn stream_table_rows_refill_writes_nothing_when_a_row_fails_to_shape() {
        let db = stored_experiments();
        let production = produced(
            vec![
                vec![experiment_json("a", "Changed")],
                vec![infer::JsonRow(vec![(
                    Arc::from("nope"),
                    serde_json::json!(1),
                )])],
            ],
            Ok(()),
        );

        let outcome = stream_table_rows(&db, "experiments", production, true).unwrap();

        let err = outcome.unwrap_err();
        assert!(err.contains("missing columns"), "got: {err}");
        assert_eq!(
            experiment_names(&db),
            vec![(1, "Alpha".into()), (2, "Beta".into())]
        );
    }

    #[test]
    fn handle_upsert_surfaces_on_file_error() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let fake = FakeFs::with_stat(abs.clone(), fake_stat());
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::try_new(
                "items",
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
    fn tables_lists_declared_tables_and_root_reports_the_index_root() {
        let dir = TempDir::new().unwrap();
        let db = DirSQL::builder()
            .root(dir.path())
            .table(Table::new(
                "b",
                "CREATE TABLE b (y TEXT)",
                "*.b",
                |_| vec![],
            ))
            .build()
            .unwrap();
        let tables = db.tables().unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "b");
        assert_eq!(tables[0].columns[0].name, "y");
        assert_eq!(db.root(), dir.path());
    }

    #[test]
    fn builder_setters_configure_and_build() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("custom-cache.db");
        let db = DirSQL::builder()
            .root(dir.path())
            .tables(vec![Table::new(
                "b",
                "CREATE TABLE b (y TEXT)",
                "*.b",
                |_| vec![],
            )])
            .table(Table::new(
                "a",
                "CREATE TABLE a (x TEXT)",
                "*.a",
                |_| vec![],
            ))
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
            db.query("SELECT path FROM './' LIMIT 1").is_ok(),
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
            .path_table_parser("cat")
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
            db.query("SELECT path FROM './' LIMIT 1").is_ok(),
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
                "t",
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
                "t",
                "CREATE TABLE t (x TEXT)",
                "*.txt",
                |_| vec![],
            )])
            .persist(Some(&cache))
            .build()
            .unwrap();
        assert!(second.query("SELECT * FROM t").is_ok());
    }

    /// A cache whose stored meta has the same key count as the expected one
    /// but a different glob hash is incompatible, so the meta is not current
    /// and the rewrite cannot be skipped. Equal length alone must not stand
    /// in for compatibility.
    #[test]
    fn prepare_persist_reports_an_incompatible_meta_of_equal_length_as_stale() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache.db");
        drop(
            DirSQL::builder()
                .root(dir.path())
                .tables(vec![Table::new(
                    "t",
                    "CREATE TABLE t (x TEXT)",
                    "*.txt",
                    |_| vec![],
                )])
                .persist(Some(&cache))
                .build()
                .unwrap(),
        );

        // `build_meta` always mints the same keys, so the stored and expected
        // blocks are the same length; only the glob hash differs.
        let tables = vec![Table::new(
            "t",
            "CREATE TABLE t (x TEXT)",
            "*.csv",
            |_| vec![],
        )];
        let ctx = prepare_persist(dir.path(), &tables, &[], Some(&cache)).unwrap();

        assert!(!ctx.meta_current, "an incompatible meta is never current");
    }

    #[test]
    fn prepare_persist_cold_start_reports_rebuild() {
        let dir = TempDir::new().unwrap();
        let tables = vec![Table::new(
            "t",
            "CREATE TABLE t (x TEXT)",
            "*.txt",
            |_| vec![],
        )];
        let ctx =
            prepare_persist(dir.path(), &tables, &[], Some(&dir.path().join("cache.db"))).unwrap();
        assert!(ctx.cached.is_empty());
        assert!(!ctx.expected_meta.is_empty());
        assert!(
            ctx.needs_sweep,
            "an incompatible cache must be handed on for sweeping"
        );
    }

    /// The counterpart: a cache whose meta matches keeps its tables, so no
    /// sweep is requested and the warm rows survive.
    #[test]
    fn prepare_persist_warm_cache_asks_for_no_sweep() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache.db");
        let tables = vec![Table::new(
            "t",
            "CREATE TABLE t (x TEXT)",
            "*.txt",
            |_| vec![],
        )];
        drop(
            DirSQL::builder()
                .root(dir.path())
                .tables(tables.clone())
                .persist(Some(&cache))
                .build()
                .unwrap(),
        );
        let ctx = prepare_persist(dir.path(), &tables, &[], Some(&cache)).unwrap();
        assert!(!ctx.needs_sweep, "a compatible cache must not be swept");
    }

    /// The sweep itself, from the build side: an edited `ddl` invalidates the
    /// cache, and only a sweep lets the batch re-run — an un-dropped `t` reads
    /// as a warm table and the new column never appears.
    #[test]
    fn an_edited_ddl_sweeps_the_cache_before_rebuilding() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache.db");
        let build = |ddl: &'static str| {
            DirSQL::builder()
                .root(dir.path())
                .tables(vec![Table::new("t", ddl, "*.txt", |_| vec![])])
                .persist(Some(&cache))
                .build()
        };
        drop(build("CREATE TABLE t (x TEXT)").unwrap());

        let second = build("CREATE TABLE t (x TEXT, y TEXT)").unwrap();
        assert!(
            second.query("SELECT y FROM t").is_ok(),
            "the rebuilt table must carry the edited batch's new column"
        );
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
            meta_current: false,
            needs_sweep: false,
        };
        let scanned = vec![found_at(dir.path(), &abs, "t")];
        let (files, deleted) = reconcile_scan(scanned, &ctx, &fake).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].trusted);
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
            meta_current: false,
            needs_sweep: false,
        };
        let scanned = vec![found_at(dir.path(), &abs, "t")];
        let (files, _deleted) = reconcile_scan(scanned, &ctx, &fake).unwrap();
        assert_eq!(files.len(), 1);
        assert!(!files[0].trusted);
    }

    #[test]
    fn reconcile_scan_reports_deleted_cached_files() {
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
            meta_current: false,
            needs_sweep: false,
        };
        let fake = FakeFs::default();
        let (files, deleted) = reconcile_scan(Vec::new(), &ctx, &fake).unwrap();
        assert!(files.is_empty());
        assert_eq!(deleted, vec![("gone.txt".to_string(), "t".to_string())]);
    }

    /// A `name` the batch never created: the error has to say what it *did*
    /// create, or a typo gives the reader nothing to compare against.
    #[test]
    fn classify_declared_table_rejects_a_name_the_batch_never_created() {
        let err = classify_declared_table(
            "messages",
            None,
            vec!["mesages_fts".to_string(), "mesages".to_string()],
        )
        .expect_err("a missing catalog entry must fail the load");

        let msg = err.to_string();
        assert!(
            matches!(err, DirSqlError::TableNotCreated { .. }),
            "got: {msg}"
        );
        assert!(msg.contains("table 'messages'"), "got: {msg}");
        assert!(
            msg.contains("it created: mesages, mesages_fts"),
            "the list must read sorted, not in the catalog's drop order, got: {msg}"
        );
    }

    #[test]
    fn table_not_created_says_nothing_when_the_batch_created_nothing() {
        let err = DirSqlError::TableNotCreated {
            name: "messages".to_string(),
            created: Vec::new(),
        };
        assert!(
            err.to_string().contains("it created: nothing"),
            "got: {err}"
        );
    }

    #[test]
    fn classify_declared_table_rejects_a_virtual_declared_table() {
        let entry = db::CatalogEntry {
            kind: "virtual".to_string(),
            without_rowid: false,
        };
        let err = classify_declared_table("messages", Some(entry), Vec::new())
            .expect_err("a virtual declared table must fail the load");

        let msg = err.to_string();
        assert!(
            matches!(err, DirSqlError::TableIsVirtual { .. }),
            "got: {msg}"
        );
        assert!(
            msg.contains("table 'messages'") && msg.contains("virtual"),
            "got: {msg}"
        );
    }

    #[test]
    fn classify_declared_table_warns_about_without_rowid() {
        let entry = db::CatalogEntry {
            kind: "table".to_string(),
            without_rowid: true,
        };
        let warning = classify_declared_table("messages", Some(entry), Vec::new())
            .expect("a WITHOUT ROWID table is a warning, not a failure")
            .expect("it must produce a warning");

        assert!(warning.contains("messages"), "got: {warning}");
        assert!(warning.contains("WITHOUT ROWID"), "got: {warning}");
    }

    #[test]
    fn classify_declared_table_accepts_a_plain_row_table() {
        let entry = db::CatalogEntry {
            kind: "table".to_string(),
            without_rowid: false,
        };
        assert_eq!(
            classify_declared_table("messages", Some(entry), Vec::new()).unwrap(),
            None
        );
    }

    #[test]
    fn table_ddl_error_prefixes_sqlites_own_message_with_the_entry() {
        let err = DirSqlError::TableDdl {
            name: "messages".to_string(),
            source: DbError::SchemaMismatch("near \"(\": syntax error".to_string()),
        };
        let msg = err.to_string();
        assert!(msg.starts_with("table 'messages': "), "got: {msg}");
        assert!(msg.contains("near \"(\": syntax error"), "got: {msg}");
    }

    #[test]
    fn build_tables_from_config_creates_on_file_tables() {
        let cfg = config::load_config_str(concat!(
            "[[table]]\n",
            "name = \"a\"\n",
            "ddl = \"CREATE TABLE a (x TEXT)\"\n",
            "glob = \"*.a\"\n",
            "on-file = \"printf '{\\\"x\\\":1}'\"\n\n",
            "[[table]]\n",
            "name = \"b\"\n",
            "ddl = \"CREATE TABLE b (y TEXT)\"\n",
            "glob = \"*.b\"\n",
            "on-file = \"echo hi\"\n",
            "strict = true\n",
        ))
        .unwrap();
        let dir = TempDir::new().unwrap();
        let tables = build_tables_from_config(&cfg, dir.path()).unwrap();
        assert_eq!(tables.len(), 2);
        let Hook::PerTable(on_files) = &tables[0].hook else {
            panic!("a configured on-file command runs once per table");
        };
        let mut rows = Vec::new();
        on_files(
            &mut std::iter::once(dir.path().join("f.a")),
            &mut |chunk| rows.extend(chunk),
            &mut || {},
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(tables[1].strict, "on-file table preserves strict flag");
    }

    #[test]
    fn build_tables_from_config_rejects_a_path_placeholder() {
        let cfg = config::load_config_str(concat!(
            "[[table]]\n",
            "name = \"a\"\n",
            "ddl = \"CREATE TABLE a (x TEXT)\"\n",
            "glob = \"*.a\"\n",
            "on-file = \"cat {path}\"\n",
        ))
        .unwrap();
        let dir = TempDir::new().unwrap();
        let err = match build_tables_from_config(&cfg, dir.path()) {
            Err(err) => err,
            Ok(_) => panic!("`{{path}}` must be rejected"),
        };
        assert!(
            matches!(&err, DirSqlError::PathPlaceholder(m) if m.contains("trailing arguments")),
            "got: {err}"
        );
    }

    fn function_spec(name: &str, timeout: Option<Duration>) -> config::FunctionSpec {
        config::FunctionSpec {
            name: name.to_string(),
            args: vec![1, 2],
            command: "worker cmd".to_string(),
            deterministic: true,
            timeout,
            batch: None,
        }
    }

    #[test]
    fn resolve_functions_maps_spec_fields_and_config_context() {
        let mut sources = HashMap::new();
        let mut out = Vec::new();
        resolve_functions(
            vec![function_spec("embed", Some(Duration::from_secs(600)))],
            Path::new("/proj/.dirsql.toml"),
            Path::new("/proj"),
            &mut sources,
            &mut out,
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "embed");
        assert_eq!(out[0].args, vec![1, 2]);
        assert_eq!(out[0].command, "worker cmd");
        assert!(out[0].deterministic);
        assert_eq!(out[0].timeout, Duration::from_secs(600));
        assert_eq!(out[0].batch, None);
        assert_eq!(out[0].cwd, PathBuf::from("/proj"));
    }

    #[test]
    fn resolve_functions_carries_the_batch_size() {
        let mut sources = HashMap::new();
        let mut out = Vec::new();
        let mut spec = function_spec("embed", None);
        spec.batch = Some(8);
        resolve_functions(
            vec![spec],
            Path::new("/proj/.dirsql.toml"),
            Path::new("/proj"),
            &mut sources,
            &mut out,
        )
        .unwrap();
        assert_eq!(out[0].batch, Some(8));
    }

    #[test]
    fn resolve_functions_defaults_the_timeout_to_thirty_seconds() {
        let mut sources = HashMap::new();
        let mut out = Vec::new();
        resolve_functions(
            vec![function_spec("embed", None)],
            Path::new("/proj/.dirsql.toml"),
            Path::new("/proj"),
            &mut sources,
            &mut out,
        )
        .unwrap();
        assert_eq!(out[0].timeout, Duration::from_secs(30));
    }

    #[test]
    fn resolve_functions_rejects_a_duplicate_name_naming_both_sources() {
        let mut sources = HashMap::new();
        let mut out = Vec::new();
        resolve_functions(
            vec![function_spec("dup", None)],
            Path::new("/a/frag.toml"),
            Path::new("/a"),
            &mut sources,
            &mut out,
        )
        .unwrap();
        let err = resolve_functions(
            vec![function_spec("dup", None)],
            Path::new("/b/frag.toml"),
            Path::new("/b"),
            &mut sources,
            &mut out,
        )
        .expect_err("duplicate function name must error");
        let msg = err.to_string();
        assert!(msg.contains("'dup'"), "got: {msg}");
        assert!(msg.contains("/a/frag.toml"), "got: {msg}");
        assert!(msg.contains("/b/frag.toml"), "got: {msg}");
        assert_eq!(out.len(), 1, "the duplicate must not be resolved");
    }

    #[test]
    fn resolve_functions_rejects_a_duplicate_within_one_config() {
        let mut sources = HashMap::new();
        let mut out = Vec::new();
        let err = resolve_functions(
            vec![function_spec("dup", None), function_spec("dup", None)],
            Path::new("/a/frag.toml"),
            Path::new("/a"),
            &mut sources,
            &mut out,
        )
        .expect_err("intra-config duplicate must error");
        assert!(err.to_string().contains("'dup'"), "got: {err}");
    }

    #[test]
    fn resolve_functions_accumulates_across_configs_in_order() {
        let mut sources = HashMap::new();
        let mut out = Vec::new();
        resolve_functions(
            vec![function_spec("fa", None)],
            Path::new("/a/frag.toml"),
            Path::new("/a"),
            &mut sources,
            &mut out,
        )
        .unwrap();
        resolve_functions(
            vec![function_spec("fb", None)],
            Path::new("/b/frag.toml"),
            Path::new("/b"),
            &mut sources,
            &mut out,
        )
        .unwrap();
        let names: Vec<&str> = out.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["fa", "fb"]);
        assert_eq!(out[1].cwd, PathBuf::from("/b"));
    }

    fn collect_on_files(
        command: &str,
        paths: &[PathBuf],
        config_dir: &Path,
        root: &Path,
    ) -> std::result::Result<Vec<infer::JsonRow>, BoxError> {
        let mut rows = Vec::new();
        run_on_files(
            command,
            &mut paths.iter().cloned(),
            config_dir,
            root,
            &mut |chunk| rows.extend(chunk),
            &mut || {},
        )?;
        Ok(rows)
    }

    #[test]
    fn run_on_files_parses_command_json_output() {
        let dir = TempDir::new().unwrap();
        let rows = collect_on_files(
            "printf '{\"n\":1}'",
            &[dir.path().join("f.txt")],
            dir.path(),
            dir.path(),
        )
        .expect("a well-formed payload parses");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("n"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn run_on_files_hands_over_each_invocations_rows_as_one_chunk() {
        let dir = TempDir::new().unwrap();
        let paths: Vec<PathBuf> = (0..3000)
            .map(|i| dir.path().join(format!("{i:0>96}.txt")))
            .collect();
        let mut chunks = Vec::new();
        run_on_files(
            r#"sh -c 'echo "{\"n\":$#}"' sh"#,
            &mut paths.iter().cloned(),
            dir.path(),
            dir.path(),
            &mut |chunk| chunks.push(chunk),
            &mut || {},
        )
        .expect("a well-formed payload parses");
        assert!(chunks.len() > 1, "{} invocation(s)", chunks.len());
        let total: u64 = chunks
            .iter()
            .map(|chunk| {
                assert_eq!(chunk.len(), 1);
                chunk[0]
                    .get("n")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap()
            })
            .sum();
        assert_eq!(total, 3000);
    }

    /// `{abspath}` is not in the substitution table: it is left literal like any
    /// unknown `{…}`, so `printf` receives the string `{abspath}` verbatim.
    #[test]
    fn run_on_files_does_not_substitute_abspath() {
        let dir = TempDir::new().unwrap();
        let rows = collect_on_files(
            r#"printf '{"q":"%s"}%.0s' {abspath}"#,
            &[dir.path().join("f.txt")],
            dir.path(),
            dir.path(),
        )
        .expect("a well-formed payload parses");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("q"), Some(&serde_json::json!("{abspath}")));
    }

    /// Every matched file's **absolute** path arrives as a trailing argument,
    /// byte-for-byte, after the user's own argv. The command writes them to a
    /// side file rather than into the JSON payload, where a Windows path's `\`
    /// would need escaping.
    #[test]
    fn run_on_files_appends_every_absolute_path_in_order() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        collect_on_files(
            r#"sh -c 'shift; printf "%s\n" "$@" > seen; true' sh first"#,
            &[a.clone(), b.clone()],
            dir.path(),
            dir.path(),
        )
        .expect("a well-formed payload parses");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seen")).unwrap(),
            format!("{}\n{}\n", a.to_string_lossy(), b.to_string_lossy())
        );
    }

    /// A verbatim Windows path reaches the hook in its plain form, both as a
    /// trailing argument and as `{root}`.
    #[test]
    fn run_on_files_hands_the_hook_non_verbatim_paths_and_root() {
        let dir = TempDir::new().unwrap();
        collect_on_files(
            r#"sh -c 'printf "%s|%s" "$2" "$1" > seen; true' sh {root}"#,
            &[PathBuf::from(r"\\?\C:\r\f.txt")],
            dir.path(),
            Path::new(r"\\?\D:\r"),
        )
        .expect("a well-formed payload parses");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("seen")).unwrap(),
            r"C:\r\f.txt|D:\r"
        );
    }

    /// A command that cannot be spawned is an error the caller records, not an
    /// empty row set: a failure and a table that legitimately produced no rows
    /// must stay distinguishable.
    #[test]
    fn run_on_files_errors_on_spawn_failure() {
        let dir = TempDir::new().unwrap();
        let error = collect_on_files(
            "definitely-not-a-real-binary-xyzzy",
            &[PathBuf::from("/outside/f.txt")],
            dir.path(),
            dir.path(),
        )
        .expect_err("an unspawnable command is a failure");
        assert!(
            error.to_string().contains("on-file command failed"),
            "the error names the stage: {error}"
        );
    }

    #[test]
    fn run_on_files_errors_on_non_json_output() {
        let dir = TempDir::new().unwrap();
        let error = collect_on_files(
            "echo not-json",
            &[PathBuf::from("/outside/f.txt")],
            dir.path(),
            dir.path(),
        )
        .expect_err("a non-JSON payload is a failure");
        assert!(
            error.to_string().contains("not one JSON object per line"),
            "the error names the stage: {error}"
        );
    }

    #[tokio::test]
    async fn async_dirsql_builds_queries_and_forwards() {
        let dir = TempDir::new().unwrap();
        let adb = AsyncDirSQL::new(
            dir.path(),
            vec![Table::new(
                "t",
                "CREATE TABLE t (x TEXT)",
                "*.none",
                |_| vec![],
            )],
        )
        .unwrap();
        adb.ready().await.unwrap();
        let rows = adb.query("SELECT 1 AS n").await.unwrap();
        assert_eq!(rows[0]["n"], Value::Integer(1));
        let bound = adb
            .query_with(
                "SELECT ? AS n",
                &Params::Positional(vec![Value::Integer(9)]),
            )
            .await
            .unwrap();
        assert_eq!(bound[0]["n"], Value::Integer(9));
        assert!(adb.query("SELECT ?").await.is_err());
        assert!(adb.sync().unwrap().query("SELECT 1").is_ok());
        assert!(
            adb.poll_events(Duration::from_millis(0))
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn async_dirsql_watch_forwards() {
        let dir = TempDir::new().unwrap();
        let adb = AsyncDirSQL::new(
            dir.path(),
            vec![Table::new(
                "t",
                "CREATE TABLE t (x TEXT)",
                "*.none",
                |_| vec![],
            )],
        )
        .unwrap();
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

    fn prepared_build(
        root: &Path,
        tables: Vec<Table>,
        scanned_files: Vec<ScannedFile>,
        persist: Option<PreparedPersist>,
    ) -> PreparedBuild {
        let scanned_files = Scanned::Files(
            scanned_files
                .into_iter()
                .map(|mut file| {
                    file.abs_path = root.join(&file.rel_path);
                    file
                })
                .collect(),
        );
        let fresh_db = persist
            .is_none()
            .then(|| (Db::new().unwrap(), false, Vec::new()));
        PreparedBuild {
            ignore: Vec::new(),
            root: root.to_path_buf(),
            tables,
            extensions: Vec::new(),
            functions: Vec::new(),
            groups: Vec::new(),
            anchors: TableAnchors::at_root(root),
            scanned_files,
            fresh_db,
            early: Vec::new(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            persist,
            hint_legacy_files_table: false,
            path_table_parser: None,
            no_ignore: false,
            links: Vec::new(),
            watcher: None,
        }
    }

    #[test]
    fn batch_paths_hands_the_hook_the_paths_the_scan_found() {
        let mut files = vec![
            ScannedFile {
                abs_path: PathBuf::from("/scan/a/one.md"),
                rel_path: "a/one.md".into(),
                table_name: "t".into(),
                stat: None,
                trusted: false,
            },
            ScannedFile {
                abs_path: PathBuf::from("/scan/b/two.md"),
                rel_path: "b/two.md".into(),
                table_name: "t".into(),
                stat: None,
                trusted: false,
            },
        ];
        let (rel, abs) = batch_paths(&mut files, true);
        assert_eq!(rel, vec!["a/one.md", "b/two.md"]);
        assert_eq!(
            abs,
            vec![
                PathBuf::from("/scan/a/one.md"),
                PathBuf::from("/scan/b/two.md")
            ]
        );
    }

    #[test]
    fn batch_paths_leaves_the_relative_paths_in_place_when_asked_to_keep_them() {
        let mut files = vec![ScannedFile {
            abs_path: PathBuf::from("/scan/a.md"),
            rel_path: "a.md".into(),
            table_name: "t".into(),
            stat: None,
            trusted: false,
        }];
        let (rel, _) = batch_paths(&mut files, true);
        assert_eq!(rel, vec!["a.md"]);
        assert_eq!(files[0].rel_path, "a.md");
    }

    #[test]
    fn batch_paths_moves_the_relative_paths_out_when_not_kept() {
        let mut files = vec![ScannedFile {
            abs_path: PathBuf::from("/scan/a.md"),
            rel_path: "a.md".into(),
            table_name: "t".into(),
            stat: None,
            trusted: false,
        }];
        let (rel, _) = batch_paths(&mut files, false);
        assert_eq!(rel, vec!["a.md"]);
        assert!(files[0].rel_path.is_empty());
    }

    fn scanned(rel_path: &str, table: &str, trusted: bool) -> ScannedFile {
        ScannedFile {
            abs_path: PathBuf::new(),
            rel_path: rel_path.into(),
            table_name: table.into(),
            stat: None,
            trusted,
        }
    }

    /// A warm cache that reports `deleted` as the files gone since its last
    /// write and whose meta is already current.
    fn warm_cache(deleted: &[(&str, &str)]) -> PreparedPersist {
        let db = Db::new().unwrap();
        create_sidecar_tables(db.conn()).unwrap();
        PreparedPersist {
            db,
            deleted: deleted
                .iter()
                .map(|(path, table)| (path.to_string(), table.to_string()))
                .collect(),
            meta: HashMap::new(),
            meta_current: true,
            needs_sweep: false,
        }
    }

    fn name_row(name: &str) -> Row {
        Row::from_iter([("name".to_string(), Value::Text(name.into()))])
    }

    fn name_json_row(name: &str) -> infer::JsonRow {
        infer::JsonRow(vec![("name".into(), serde_json::json!(name))])
    }

    type Calls = Arc<Mutex<Vec<Vec<PathBuf>>>>;

    /// A per-table hook that records every argument list it is handed and
    /// answers one `name` row per path, holding the file's basename.
    fn recording_per_table(name: &str, glob: &str) -> (Table, Calls) {
        let calls: Calls = Arc::default();
        let seen = Arc::clone(&calls);
        let table = Table::per_table(
            name,
            format!("CREATE TABLE {name} (name TEXT)"),
            glob,
            move |paths: &[PathBuf]| {
                seen.lock().unwrap().push(paths.to_vec());
                Ok(paths
                    .iter()
                    .map(|p| name_json_row(&p.file_name().unwrap().to_string_lossy()))
                    .collect())
            },
        );
        (table, calls)
    }

    fn build(prepared: PreparedBuild) -> DirSQL {
        DirSQL::finish_build_with_fs(prepared, Arc::new(FakeFs::default()), Progress::indexing())
            .unwrap()
    }

    #[test]
    fn declared_position_is_a_tables_index_in_declaration_order() {
        let declared = ["zeta".to_string(), "alpha".to_string()];
        assert_eq!(declared_position(&declared, "zeta"), Some(0));
        assert_eq!(declared_position(&declared, "alpha"), Some(1));
        assert_eq!(declared_position(&declared, "beta"), None);
    }

    #[test]
    fn finish_build_runs_a_per_table_hook_once_over_its_files_in_scan_order() {
        let dir = TempDir::new().unwrap();
        let (items, calls) = recording_per_table("items", "*.txt");
        let scanned = vec![
            scanned("b.txt", "items", false),
            scanned("a.txt", "items", false),
        ];

        let db = build(prepared_build(dir.path(), vec![items], scanned, None));

        assert_eq!(
            *calls.lock().unwrap(),
            vec![vec![dir.path().join("b.txt"), dir.path().join("a.txt")]]
        );
        assert_eq!(
            db.query("SELECT name FROM items ORDER BY rowid").unwrap(),
            vec![name_row("b.txt"), name_row("a.txt")]
        );
        assert!(db.scan_failures().is_empty(), "{:?}", db.scan_failures());
    }

    fn streaming_per_table(
        hook: impl Fn(&mut RowSink<'_>) -> std::result::Result<(), BoxError> + Send + Sync + 'static,
    ) -> Table {
        Table::per_table_streaming(
            "items",
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            move |_: &mut PathFeed<'_>, sink: &mut RowSink<'_>, _: &mut OutputEnd<'_>| hook(sink),
        )
    }

    fn table_command_error(table: Table, dir: &Path) -> String {
        let prepared = prepared_build(
            dir,
            vec![table],
            vec![scanned("a.txt", "items", false)],
            None,
        );
        match DirSQL::finish_build_with_fs(
            prepared,
            Arc::new(FakeFs::default()),
            Progress::indexing(),
        ) {
            Err(DirSqlError::TableCommand { message, .. }) => message,
            Err(other) => panic!("expected a table command error, got {other}"),
            Ok(_) => panic!("expected the build to fail"),
        }
    }

    #[test]
    fn finish_build_stores_every_chunk_a_per_table_hook_emits_in_order() {
        let dir = TempDir::new().unwrap();
        let items = streaming_per_table(|sink| {
            sink(vec![name_json_row("a"), name_json_row("b")]);
            sink(vec![name_json_row("c")]);
            Ok(())
        });

        let db = build(prepared_build(
            dir.path(),
            vec![items],
            vec![scanned("a.txt", "items", false)],
            None,
        ));

        assert_eq!(
            db.query("SELECT name FROM items ORDER BY rowid").unwrap(),
            vec![name_row("a"), name_row("b"), name_row("c")]
        );
    }

    #[test]
    fn finish_build_fails_a_table_whose_chunk_does_not_shape() {
        let dir = TempDir::new().unwrap();
        let mut items = streaming_per_table(|sink| {
            sink(vec![name_json_row("a")]);
            sink(vec![infer::JsonRow(vec![(
                "nope".into(),
                serde_json::json!(1),
            )])]);
            Ok(())
        });
        items.strict = true;

        let message = table_command_error(items, dir.path());

        assert!(message.contains("extra columns"), "{message}");
    }

    #[test]
    fn finish_build_reports_a_hook_failure_over_an_earlier_chunk_that_did_not_shape() {
        let dir = TempDir::new().unwrap();
        let mut items = streaming_per_table(|sink| {
            sink(vec![infer::JsonRow(vec![(
                "nope".into(),
                serde_json::json!(1),
            )])]);
            Err("parser exploded".into())
        });
        items.strict = true;

        let message = table_command_error(items, dir.path());

        assert_eq!(message, "parser exploded");
    }

    #[test]
    fn finish_build_owns_a_per_table_hooks_rows_as_one_range_not_a_mapping_row_each() {
        let dir = TempDir::new().unwrap();
        let (items, _) = recording_per_table("items", "*.txt");
        let scanned = vec![
            scanned("a.txt", "items", false),
            scanned("b.txt", "items", false),
            scanned("c.txt", "items", false),
        ];

        let db = build(prepared_build(dir.path(), vec![items], scanned, None));

        assert_eq!(row_names(&db), vec!["a.txt", "b.txt", "c.txt"]);
        let mapped: i64 = db
            .inner
            .db
            .lock()
            .unwrap()
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM _dirsql_internal_rows WHERE table_name = 'items'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            mapped, 0,
            "a per-table hook's rows are owned by one rowid range, not a mapping row each"
        );
    }

    /// A `Write` the test reads back, standing in for stderr.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn finish_build_counts_per_file_and_per_table_files_toward_one_ingest_total() {
        let dir = TempDir::new().unwrap();
        let out = Captured::default();
        let progress = Progress::new(
            "indexing",
            "indexed",
            "files",
            Box::new(out.clone()),
            Box::new(progress::SystemClock),
            progress::Mode::Always,
            false,
        );
        let notes = Table::new("notes", "CREATE TABLE notes (name TEXT)", "*.md", |_| {
            Vec::new()
        });
        let (items, _) = recording_per_table("items", "*.txt");
        let (tags, _) = recording_per_table("tags", "*.tag");
        let scanned = vec![
            scanned("n.md", "notes", false),
            scanned("a.txt", "items", false),
            scanned("b.txt", "items", false),
            scanned("t.tag", "tags", false),
        ];
        let prepared = prepared_build(dir.path(), vec![notes, items, tags], scanned, None);

        DirSQL::finish_build_with_fs(prepared, Arc::new(FakeFs::default()), progress).unwrap();

        let text = out.text();
        for line in [
            "dirsql: indexing 0/4 files (0%)",
            "dirsql: indexing 1/4 files (25%)",
            "dirsql: indexing 3/4 files (75%)",
            "dirsql: indexed 4 files in",
        ] {
            assert!(text.contains(line), "missing {line:?} in {text:?}");
        }
    }

    #[test]
    fn finish_build_counts_the_files_of_a_hook_started_before_the_scan_toward_the_ingest_total() {
        let dir = TempDir::new().unwrap();
        let out = Captured::default();
        let progress = Progress::new(
            "indexing",
            "indexed",
            "files",
            Box::new(out.clone()),
            Box::new(progress::SystemClock),
            progress::Mode::Always,
            false,
        );
        let notes = Table::new("notes", "CREATE TABLE notes (name TEXT)", "*.md", |_| {
            Vec::new()
        });
        let (items, _) = recording_per_table("items", "*.txt");
        let (feeds, early) = start_per_table_hooks(std::slice::from_ref(&items));
        for name in ["a.txt", "b.txt"] {
            feeds["items"].send(dir.path().join(name)).unwrap();
        }
        drop(feeds);
        let scanned = vec![
            scanned("a.txt", "items", false),
            scanned("b.txt", "items", false),
            scanned("n.md", "notes", false),
        ];
        let mut prepared = prepared_build(dir.path(), vec![notes, items], scanned, None);
        prepared.early = early;

        DirSQL::finish_build_with_fs(prepared, Arc::new(FakeFs::default()), progress).unwrap();

        let text = out.text();
        for line in [
            "dirsql: indexing 0/3 files (0%)",
            "dirsql: indexing 2/3 files (66%)",
            "dirsql: indexed 3 files in",
        ] {
            assert!(text.contains(line), "missing {line:?} in {text:?}");
        }
    }

    #[test]
    fn finish_build_skips_a_per_file_hook_for_a_file_the_cache_holds() {
        let dir = TempDir::new().unwrap();
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = Arc::clone(&calls);
        let notes = Table::new(
            "notes",
            "CREATE TABLE notes (name TEXT)",
            "*.md",
            move |path| {
                seen.lock().unwrap().push(path.to_string());
                Vec::new()
            },
        );
        let scanned = vec![
            scanned("cached.md", "notes", true),
            scanned("fresh.md", "notes", false),
        ];

        build(prepared_build(
            dir.path(),
            vec![notes],
            scanned,
            Some(warm_cache(&[])),
        ));

        assert_eq!(
            *calls.lock().unwrap(),
            vec![dir.path().join("fresh.md").to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn finish_build_keeps_a_per_table_hooks_cached_rows_while_every_file_is_current() {
        let dir = TempDir::new().unwrap();
        let notes = Table::new("notes", "CREATE TABLE notes (name TEXT)", "*.md", |_| {
            Vec::new()
        });
        let (items, calls) = recording_per_table("items", "*.txt");
        let scanned = vec![
            scanned("a.txt", "items", true),
            scanned("b.txt", "items", true),
        ];

        build(prepared_build(
            dir.path(),
            vec![notes, items],
            scanned,
            Some(warm_cache(&[("gone.md", "notes")])),
        ));

        assert!(
            calls.lock().unwrap().is_empty(),
            "{:?}",
            calls.lock().unwrap()
        );
    }

    #[test]
    fn finish_build_reruns_a_per_table_hook_when_one_of_its_files_is_stale() {
        let dir = TempDir::new().unwrap();
        let (items, calls) = recording_per_table("items", "*.txt");
        let scanned = vec![
            scanned("a.txt", "items", true),
            scanned("b.txt", "items", false),
        ];

        build(prepared_build(
            dir.path(),
            vec![items],
            scanned,
            Some(warm_cache(&[])),
        ));

        assert_eq!(
            *calls.lock().unwrap(),
            vec![vec![dir.path().join("a.txt"), dir.path().join("b.txt")]]
        );
    }

    #[test]
    fn finish_build_reruns_a_per_table_hook_when_one_of_its_files_is_gone() {
        let dir = TempDir::new().unwrap();
        let (items, calls) = recording_per_table("items", "*.txt");
        let scanned = vec![scanned("a.txt", "items", true)];

        build(prepared_build(
            dir.path(),
            vec![items],
            scanned,
            Some(warm_cache(&[("gone.txt", "items")])),
        ));

        assert_eq!(*calls.lock().unwrap(), vec![vec![dir.path().join("a.txt")]]);
    }

    #[test]
    fn apply_file_events_reruns_a_per_table_hook_once_for_a_burst_of_its_files() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        let fake = FakeFs::default().with_subtree(
            root.clone(),
            vec![(a.clone(), "items".into()), (b.clone(), "items".into())],
        );
        let (items, calls) = recording_per_table("items", "**/*.txt");
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), Arc::new(fake))
                .unwrap();
        assert!(
            calls.lock().unwrap().is_empty(),
            "a table with no files spawns nothing"
        );

        let events = db.apply_file_events(vec![
            FileEvent::Created(a.clone()),
            FileEvent::Modified(b.clone()),
        ]);

        assert_eq!(*calls.lock().unwrap(), vec![vec![a, b]]);
        assert_eq!(
            events,
            vec![
                RowEvent::Insert {
                    table: "items".into(),
                    row: name_row("a.txt"),
                    file_path: "a.txt".into(),
                },
                RowEvent::Insert {
                    table: "items".into(),
                    row: name_row("b.txt"),
                    file_path: "a.txt".into(),
                },
            ]
        );
        assert_eq!(row_names(&db), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn startup_runs_a_per_table_hook_once_over_every_file() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("a.txt"), "").unwrap();
        std::fs::write(root.join("b.txt"), "").unwrap();
        let (items, calls) = recording_per_table("items", "*.txt");

        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), Arc::new(RealFs))
                .unwrap();

        {
            let calls = calls.lock().unwrap();
            assert_eq!(calls.len(), 1, "{calls:?}");
            let mut seen = calls[0].clone();
            seen.sort();
            assert_eq!(seen, vec![root.join("a.txt"), root.join("b.txt")]);
        }
        let mut names = row_names(&db);
        names.sort();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn apply_file_events_refreshes_a_per_table_hook_without_walking_the_tree() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let a = root.join("a.txt");
        let b = root.join("b.txt");
        let fake = Arc::new(FakeFs::default().with_subtree(
            root.clone(),
            vec![(a.clone(), "items".into()), (b.clone(), "items".into())],
        ));
        let (items, calls) = recording_per_table("items", "**/*.txt");
        let db = DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), fake.clone())
            .unwrap();

        db.apply_file_events(vec![
            FileEvent::Created(b.clone()),
            FileEvent::Created(a.clone()),
        ]);
        db.apply_file_events(vec![FileEvent::Modified(a.clone())]);
        db.apply_file_events(vec![FileEvent::Deleted(a.clone())]);

        assert_eq!(
            fake.subtree_walks(),
            Vec::<PathBuf>::new(),
            "a file event must not re-walk the tree"
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec![vec![a.clone(), b.clone()], vec![a, b.clone()], vec![b]]
        );
    }

    /// A per-table hook that answers `answer`'s name, ends its output, and
    /// then fails when `answer` says so.
    fn ending_per_table(answer: Arc<Mutex<(String, bool)>>) -> Table {
        Table::per_table_streaming(
            "items",
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            move |_: &mut PathFeed<'_>, sink: &mut RowSink<'_>, end: &mut OutputEnd<'_>| {
                let (name, fails) = answer.lock().unwrap().clone();
                sink(vec![name_json_row(&name)]);
                end();
                if fails {
                    return Err("exited 1".into());
                }
                Ok(())
            },
        )
    }

    fn ending_hook_db(answer: &Arc<Mutex<(String, bool)>>) -> (TempDir, DirSQL, PathBuf) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let a = root.join("a.txt");
        let fake = Arc::new(
            FakeFs::default().with_subtree(root.clone(), vec![(a.clone(), "items".into())]),
        );
        let items = ending_per_table(Arc::clone(answer));
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), fake).unwrap();
        db.apply_file_events(vec![FileEvent::Created(a.clone())]);
        (dir, db, a)
    }

    #[test]
    fn a_per_table_hook_that_fails_after_ending_its_output_writes_nothing() {
        let answer = Arc::new(Mutex::new(("old".to_string(), false)));
        let (_dir, db, a) = ending_hook_db(&answer);
        *answer.lock().unwrap() = ("new".to_string(), true);

        let events = db.apply_file_events(vec![FileEvent::Modified(a)]);

        assert!(
            matches!(&events[..], [RowEvent::Error { error, .. }] if error.contains("exited 1")),
            "{events:?}"
        );
        assert_eq!(row_names(&db), vec!["old"]);
    }

    #[test]
    fn a_per_table_hook_that_succeeds_after_ending_its_output_keeps_its_rows() {
        let answer = Arc::new(Mutex::new(("old".to_string(), false)));
        let (_dir, db, a) = ending_hook_db(&answer);
        *answer.lock().unwrap() = ("new".to_string(), false);

        let events = db.apply_file_events(vec![FileEvent::Modified(a)]);

        assert!(
            matches!(&events[..], [RowEvent::Update { .. }]),
            "{events:?}"
        );
        assert_eq!(row_names(&db), vec!["new"]);
    }

    #[test]
    fn apply_file_events_writes_only_the_rows_a_per_table_refresh_changed() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let paths: Vec<PathBuf> = (0..20).map(|i| root.join(format!("f{i:02}.txt"))).collect();
        let found = paths.iter().map(|p| (p.clone(), "items".into())).collect();
        let fake = Arc::new(FakeFs::default().with_subtree(root.clone(), found));
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        let items = Table::per_table(
            "items",
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            move |paths: &[PathBuf]| {
                let run = counted.fetch_add(1, Ordering::SeqCst);
                Ok(paths
                    .iter()
                    .map(|p| {
                        let name = p.file_name().unwrap().to_string_lossy().into_owned();
                        name_json_row(&if name == "f00.txt" {
                            format!("{name}#{run}")
                        } else {
                            name
                        })
                    })
                    .collect())
            },
        );
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), fake).unwrap();
        db.apply_file_events(paths.iter().cloned().map(FileEvent::Created).collect());
        let written = || db.inner.db.lock().unwrap().conn().total_changes();
        let before = written();

        let events = db.apply_file_events(vec![FileEvent::Modified(paths[0].clone())]);

        assert_eq!(events.len(), 1, "{events:?}");
        let delta = written() - before;
        assert!(delta <= 6, "one changed row rewrote {delta} rows");
        assert_eq!(row_names(&db).len(), 20);
    }

    fn walked_paths(root: &Path, glob: &str) -> Vec<PathBuf> {
        let matcher = TableMatcher::new(&[(glob, "items")], &[]).unwrap();
        scanner::scan_subtree(root, root, &matcher)
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    }

    #[test]
    fn a_per_table_hook_gets_the_argument_list_a_walk_of_the_tree_gives_it() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let write = |rel: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
            path
        };
        let (items, calls) = recording_per_table("items", "**/*.txt");
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), Arc::new(RealFs))
                .unwrap();
        let last_args = || calls.lock().unwrap().last().cloned().unwrap();

        let events: Vec<FileEvent> = [
            "z.txt",
            "a-b.txt",
            "a/c.txt",
            ".hid.txt",
            "a/b/d.txt",
            "n.md",
        ]
        .into_iter()
        .map(|rel| FileEvent::Created(write(rel)))
        .collect();
        db.apply_file_events(events);
        assert_eq!(last_args(), walked_paths(&root, "**/*.txt"));

        write("a/c.txt");
        db.apply_file_events(vec![FileEvent::Modified(root.join("a/c.txt"))]);
        assert_eq!(last_args(), walked_paths(&root, "**/*.txt"));

        write("m/one.txt");
        write("m/deep/two.txt");
        db.apply_file_events(vec![FileEvent::Created(root.join("m"))]);
        assert_eq!(last_args(), walked_paths(&root, "**/*.txt"));

        std::fs::remove_file(root.join("a-b.txt")).unwrap();
        db.apply_file_events(vec![FileEvent::Deleted(root.join("a-b.txt"))]);
        assert_eq!(last_args(), walked_paths(&root, "**/*.txt"));

        std::fs::remove_dir_all(root.join("m")).unwrap();
        db.apply_file_events(vec![FileEvent::Deleted(root.join("m"))]);
        assert_eq!(last_args(), walked_paths(&root, "**/*.txt"));
    }

    #[test]
    fn a_vanished_file_is_dropped_from_a_per_table_hooks_arguments() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (items, calls) = recording_per_table("items", "*.txt");
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), Arc::new(RealFs))
                .unwrap();
        std::fs::write(root.join("a.txt"), "x").unwrap();
        db.apply_file_events(vec![FileEvent::Created(root.join("a.txt"))]);
        std::fs::remove_file(root.join("a.txt")).unwrap();

        db.apply_file_events(vec![FileEvent::Modified(root.join("a.txt"))]);

        assert_eq!(
            calls.lock().unwrap().last().unwrap(),
            &Vec::<PathBuf>::new()
        );
        assert_eq!(row_names(&db), Vec::<String>::new());
    }

    fn rescan_fixture(per_table: bool) -> (TempDir, PathBuf, DirSQL, Calls) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (items, calls) = recording_per_table("items", "**/*.txt");
        let table = if per_table {
            items
        } else {
            Table::new(
                "items",
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                |path| {
                    vec![Row::from_iter([(
                        "name".to_string(),
                        Value::Text(
                            Path::new(path)
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )])]
                },
            )
        };
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![table], Vec::<String>::new(), Arc::new(RealFs))
                .unwrap();
        for rel in ["a.txt", "sub/b.txt"] {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
            db.apply_file_events(vec![FileEvent::Created(path)]);
        }
        (dir, root, db, calls)
    }

    fn drift(root: &Path) {
        std::fs::remove_file(root.join("a.txt")).unwrap();
        std::fs::write(root.join("sub/c.txt"), "x").unwrap();
        std::fs::write(root.join("d.txt"), "x").unwrap();
    }

    fn name_of(row: &Row) -> String {
        match &row["name"] {
            Value::Text(n) => n.clone(),
            other => format!("{other:?}"),
        }
    }

    fn rescan_summary(events: &[RowEvent]) -> Vec<String> {
        let mut out: Vec<String> = events
            .iter()
            .map(|event| match event {
                RowEvent::Insert { row, .. } => format!("insert {}", name_of(row)),
                RowEvent::Delete { row, .. } => format!("delete {}", name_of(row)),
                other => format!("{other:?}"),
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn a_rescan_forgets_the_failures_it_reads_again() {
        let (_dir, _root, db, _) = rescan_fixture(false);
        db.record_read("items", "gone.txt", Some("broken"));

        db.apply_file_events(vec![FileEvent::Rescan]);

        assert_eq!(failing_paths(&db), vec![]);
    }

    #[test]
    fn a_rescan_reconciles_a_per_file_table_with_the_tree() {
        let (_dir, root, db, _) = rescan_fixture(false);
        drift(&root);

        let events = db.apply_file_events(vec![FileEvent::Rescan]);

        assert_eq!(
            rescan_summary(&events),
            vec!["delete a.txt", "insert c.txt", "insert d.txt"]
        );
        assert_eq!(stored_names(&db), vec!["b.txt", "c.txt", "d.txt"]);
    }

    #[test]
    fn a_rescan_rebuilds_a_per_table_hooks_files_and_reconciles_its_rows() {
        let (_dir, root, db, calls) = rescan_fixture(true);
        drift(&root);

        let events = db.apply_file_events(vec![FileEvent::Rescan]);

        assert_eq!(
            calls.lock().unwrap().last().unwrap(),
            &vec![
                root.join("d.txt"),
                root.join("sub/b.txt"),
                root.join("sub/c.txt")
            ]
        );
        assert_eq!(
            rescan_summary(&events),
            vec![
                "Update { table: \"items\", old_row: {\"name\": Text(\"a.txt\")}, new_row: {\"name\": Text(\"d.txt\")}, file_path: \".\" }",
                "insert c.txt"
            ]
        );
        assert_eq!(stored_names(&db), vec!["b.txt", "c.txt", "d.txt"]);
    }

    #[test]
    fn a_rescan_of_a_tree_that_did_not_drift_emits_nothing() {
        let (_dir, _root, db, _) = rescan_fixture(false);
        assert_eq!(db.apply_file_events(vec![FileEvent::Rescan]), vec![]);
    }

    fn two_query_errors(db: &DirSQL) -> Vec<DirSqlError> {
        (0..2)
            .map(|_| match db.query("SELECT name FROM items") {
                Ok(rows) => panic!("the query returned {rows:?}"),
                Err(e) => e,
            })
            .collect()
    }

    #[test]
    fn an_overflow_fails_every_query_after_it() {
        let (_dir, _root, db, _) = rescan_fixture(false);

        db.apply_file_events(vec![FileEvent::Rescan]);

        for err in two_query_errors(&db) {
            assert!(matches!(err, DirSqlError::Watch { .. }), "got: {err:?}");
        }
    }

    #[test]
    fn a_directory_over_the_watch_limit_fails_every_query_after_it() {
        let (_dir, _root, db, _) = rescan_fixture(false);

        db.apply_file_events(vec![FileEvent::Fault(WatchFault::Limit)]);

        for err in two_query_errors(&db) {
            assert_eq!(err.os_error().map(|os| os.errno), Some(28), "got: {err:?}");
            assert_eq!(err.to_string(), "inotify watch limit reached");
        }
    }

    #[test]
    fn a_directory_that_cannot_be_watched_fails_every_query_after_it() {
        let (_dir, _root, db, _) = rescan_fixture(false);

        db.apply_file_events(vec![FileEvent::Fault(WatchFault::Failed(
            "denied".to_string(),
        ))]);

        for err in two_query_errors(&db) {
            assert_eq!(err.to_string(), "watcher error: denied");
        }
    }

    #[test]
    fn a_build_over_the_watch_limit_raises_what_watchdog_raises() {
        let err = match watcher_at_build::<()>(Err(max_files_watch_error())) {
            Ok(kept) => panic!("the build kept {kept:?}"),
            Err(e) => e,
        };
        let DirSqlError::Io(io) = &err else {
            panic!("not an io error: {err:?}");
        };
        assert_eq!(io.kind(), std::io::ErrorKind::StorageFull);
        assert_eq!(
            err.os_error(),
            Some(&OsError {
                errno: 28,
                code: "ENOSPC",
                message: "inotify watch limit reached",
            })
        );
        assert_eq!(err.to_string(), "inotify watch limit reached");
    }

    #[test]
    fn a_build_whose_watcher_cannot_start_fails_with_the_watch_error() {
        let err = match watcher_at_build::<()>(Err(generic_watch_error("denied"))) {
            Ok(kept) => panic!("the build kept {kept:?}"),
            Err(e) => e,
        };
        assert_eq!(err.to_string(), "watcher error: denied");
    }

    #[test]
    fn a_build_over_a_missing_anchor_starts_no_watcher() {
        assert_eq!(
            watcher_at_build::<()>(Err(path_not_found_error())).unwrap(),
            None
        );
        assert_eq!(watcher_at_build(Ok(7)).unwrap(), Some(7));
    }

    fn anchored_table(name: &str, glob: &str, anchor: &str) -> Table {
        Table::new(
            name,
            format!("CREATE TABLE {name} (x TEXT)"),
            glob,
            |_| vec![],
        )
        .anchored(anchor)
    }

    #[test]
    fn compile_groups_puts_unanchored_tables_at_the_index_root() {
        let tables = vec![Table::new(
            "a",
            "CREATE TABLE a (x TEXT)",
            "*.txt",
            |_| vec![],
        )];
        let (groups, anchors, names) = compile_groups(&tables, &[], Path::new("/idx")).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].anchor, PathBuf::from("/idx"));
        assert_eq!(groups[0].tables, vec!["a".to_string()]);
        assert_eq!(anchors.of("a"), Path::new("/idx"));
        assert_eq!(names, vec!["a".to_string()]);
    }

    #[test]
    fn compile_groups_makes_one_group_per_distinct_anchor() {
        let tables = vec![
            anchored_table("a", "*.txt", "/one"),
            anchored_table("b", "*.md", "/two"),
            anchored_table("c", "*.rs", "/one"),
        ];
        let (groups, anchors, _) = compile_groups(&tables, &[], Path::new("/idx")).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].anchor, PathBuf::from("/one"));
        assert_eq!(groups[0].tables, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(groups[1].anchor, PathBuf::from("/two"));
        assert_eq!(anchors.of("b"), Path::new("/two"));
        assert_eq!(anchors.of("c"), Path::new("/one"));
    }

    #[test]
    fn compile_groups_rejects_a_duplicate_name_across_anchors() {
        let tables = vec![
            anchored_table("a", "*.txt", "/one"),
            anchored_table("a", "*.md", "/two"),
        ];
        let err = compile_groups(&tables, &[], Path::new("/idx"))
            .err()
            .unwrap();
        assert!(matches!(err, DirSqlError::DuplicateTable(ref name) if name == "a"));
    }

    #[test]
    fn an_empty_warning_table_is_named_with_its_glob_and_anchor() {
        let tables = vec![anchored_table("a", "*.txt", "/one").warning_when_empty()];
        let anchors = compile_groups(&tables, &[], Path::new("/idx")).unwrap().1;
        assert_eq!(
            empty_table_warnings(&tables, &anchors, &[]),
            vec!["dirsql: table 'a': glob '*.txt' matched no files under /one".to_string()]
        );
    }

    #[test]
    fn a_table_with_a_matched_file_does_not_warn() {
        let tables = vec![anchored_table("a", "*.txt", "/one").warning_when_empty()];
        let anchors = compile_groups(&tables, &[], Path::new("/idx")).unwrap().1;
        let scanned = vec![found_at(Path::new("/one"), Path::new("/one/x.txt"), "a")];
        assert!(empty_table_warnings(&tables, &anchors, &scanned).is_empty());
    }

    #[test]
    fn a_programmatic_empty_table_does_not_warn() {
        let tables = vec![anchored_table("a", "*.txt", "/one")];
        let anchors = compile_groups(&tables, &[], Path::new("/idx")).unwrap().1;
        assert!(empty_table_warnings(&tables, &anchors, &[]).is_empty());
    }

    #[test]
    fn anchors_of_an_unknown_table_is_the_index_root() {
        let anchors = TableAnchors::at_root(Path::new("/idx"));
        assert_eq!(anchors.of("nope"), Path::new("/idx"));
    }

    #[test]
    fn an_event_under_one_anchor_is_not_offered_to_another() {
        let dir = TempDir::new().unwrap();
        let one = dir.path().join("one");
        let two = dir.path().join("two");
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![
                anchored_table("a", "*.txt", one.to_str().unwrap()),
                anchored_table("b", "*.txt", two.to_str().unwrap()),
            ],
            Vec::<String>::new(),
            Arc::new(FakeFs::default()),
        )
        .unwrap();
        let hits = db.groups_containing(&one.join("x.txt"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0.tables, vec!["a".to_string()]);
        assert_eq!(hits[0].1, PathBuf::from("x.txt"));
    }

    #[test]
    fn an_event_under_no_anchor_is_offered_raw_to_every_group() {
        let dir = TempDir::new().unwrap();
        let one = dir.path().join("one");
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![anchored_table("a", "*.txt", one.to_str().unwrap())],
            Vec::<String>::new(),
            Arc::new(FakeFs::default()),
        )
        .unwrap();
        let hits = db.groups_containing(Path::new("/elsewhere/x.txt"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, PathBuf::from("/elsewhere/x.txt"));
    }

    fn scopes_for(anchors: &[&str], ignore: &[&str]) -> Vec<WatchScope> {
        let tables: Vec<Table> = anchors
            .iter()
            .enumerate()
            .map(|(i, anchor)| anchored_table(&format!("t{i}"), "*.txt", anchor))
            .collect();
        let ignore: Vec<String> = ignore.iter().map(|p| p.to_string()).collect();
        let (groups, _, _) = compile_groups(&tables, &ignore, Path::new("/idx")).unwrap();
        watch_scopes(&groups)
    }

    fn roots(scopes: &[WatchScope]) -> Vec<PathBuf> {
        scopes.iter().map(|scope| scope.root.clone()).collect()
    }

    #[test]
    fn a_built_instance_watches_its_tables_anchor() {
        let dir = TempDir::new().unwrap();
        let db = populated_db(dir.path(), "**/*.txt", &["a.txt"]);

        assert_eq!(
            roots(&db.watch_scopes()),
            vec![db.inner.groups[0].watch_anchor.clone()]
        );
    }

    #[test]
    fn watch_scopes_lists_each_anchor_under_its_skip_rules() {
        let scopes = scopes_for(&["/one", "/one/deep"], &["node_modules/**"]);
        assert_eq!(
            roots(&scopes),
            vec![PathBuf::from("/one"), PathBuf::from("/one/deep")]
        );
        assert!(
            scopes
                .iter()
                .all(|scope| scope.ignore.is_ignored_dir(Path::new("node_modules")))
        );
    }

    /// The digit a fixture file name ends in, as an integer.
    fn trailing_id(path: &str) -> Value {
        Value::Integer(path[path.len() - 1..].parse().unwrap())
    }

    /// `parent` rows from `p<id>` files and `child` rows from `c<id>` files,
    /// each child referencing the parent with its id. `files` are applied;
    /// `unseen` exist but have not been reported yet.
    fn fk_db(root: &Path, files: &[&str], unseen: &[&str]) -> (Arc<FakeFs>, DirSQL) {
        let mut fake = FakeFs::default();
        for f in files.iter().chain(unseen) {
            fake.stats.insert(root.join(f), fake_stat());
        }
        let fake = Arc::new(fake);
        let db = DirSQL::with_ignore_and_fs(
            root,
            vec![
                Table::new(
                    "child",
                    "CREATE TABLE child (pid INTEGER REFERENCES parent(id))",
                    "c*",
                    |path| vec![Row::from_iter([("pid".to_string(), trailing_id(path))])],
                ),
                Table::new(
                    "parent",
                    "CREATE TABLE parent (id INTEGER PRIMARY KEY)",
                    "p*",
                    |path| vec![Row::from_iter([("id".to_string(), trailing_id(path))])],
                ),
            ],
            Vec::<String>::new(),
            fake.clone(),
        )
        .unwrap();
        let events = db.apply_file_events(
            files
                .iter()
                .map(|f| FileEvent::Created(root.join(f)))
                .collect(),
        );
        assert_eq!(events.len(), files.len(), "got: {events:?}");
        (fake, db)
    }

    fn parent_ids(db: &DirSQL) -> Vec<Value> {
        db.query("SELECT id FROM parent ORDER BY id")
            .unwrap()
            .into_iter()
            .map(|row| row["id"].clone())
            .collect()
    }

    fn error_count(events: &[RowEvent]) -> usize {
        events
            .iter()
            .filter(|event| matches!(event, RowEvent::Error { .. }))
            .count()
    }

    fn held(db: &DirSQL) -> Vec<FileEvent> {
        db.inner.held.lock().unwrap().clone()
    }

    #[test]
    fn a_rejected_batch_is_held_until_a_batch_resolves_it() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let (fake, db) = fk_db(root, &["p1", "p2", "c1"], &[]);
        fake.remove(&root.join("p1"));

        let rejected = db.apply_file_events(vec![FileEvent::Deleted(root.join("p1"))]);
        assert_eq!(error_count(&rejected), 1, "got: {rejected:?}");
        assert_eq!(rejected.len(), 1, "got: {rejected:?}");
        assert_eq!(held(&db), vec![FileEvent::Deleted(root.join("p1"))]);
        assert_eq!(db.apply_file_events(Vec::new()), Vec::new());
        assert_eq!(held(&db).len(), 1, "an empty poll replays nothing");

        fake.remove(&root.join("c1"));
        let events = db.apply_file_events(vec![FileEvent::Deleted(root.join("c1"))]);
        assert_eq!(error_count(&events), 0, "got: {events:?}");
        assert_eq!(events.len(), 2, "got: {events:?}");
        assert_eq!(parent_ids(&db), vec![Value::Integer(2)]);
        assert_eq!(held(&db), Vec::new());
    }

    #[test]
    fn a_batch_lands_alone_while_a_held_one_still_fails() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let (fake, db) = fk_db(root, &["p1", "c1"], &["p3"]);
        fake.remove(&root.join("p1"));
        db.apply_file_events(vec![FileEvent::Deleted(root.join("p1"))]);

        let events = db.apply_file_events(vec![FileEvent::Created(root.join("p3"))]);

        assert_eq!(error_count(&events), 1, "the held rejection: {events:?}");
        assert_eq!(events.len(), 2, "the error and p3's insert: {events:?}");
        assert_eq!(held(&db), vec![FileEvent::Deleted(root.join("p1"))]);
        assert_eq!(parent_ids(&db), vec![Value::Integer(1), Value::Integer(3)]);
    }

    #[test]
    fn a_held_batch_and_a_failing_one_are_both_held() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let (fake, db) = fk_db(root, &["p1", "p2", "c1", "c2"], &[]);
        fake.remove(&root.join("p1"));
        db.apply_file_events(vec![FileEvent::Deleted(root.join("p1"))]);
        fake.remove(&root.join("p2"));

        let events = db.apply_file_events(vec![FileEvent::Deleted(root.join("p2"))]);

        assert_eq!(error_count(&events), 1, "got: {events:?}");
        assert_eq!(events.len(), 1, "got: {events:?}");
        assert_eq!(
            held(&db),
            vec![
                FileEvent::Deleted(root.join("p1")),
                FileEvent::Deleted(root.join("p2"))
            ]
        );
        assert_eq!(parent_ids(&db), vec![Value::Integer(1), Value::Integer(2)]);
    }

    #[test]
    fn hold_keeps_each_event_once() {
        let dir = TempDir::new().unwrap();
        let (_fake, db) = fk_db(dir.path(), &[], &[]);
        let a = FileEvent::Modified(dir.path().join("a"));
        db.hold(vec![a.clone(), FileEvent::Rescan]);
        db.hold(vec![FileEvent::Rescan, a.clone()]);
        assert_eq!(held(&db), vec![a, FileEvent::Rescan]);
    }

    #[test]
    fn take_held_restates_each_path_as_it_is_now() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let mut fake = FakeFs::default().with_dir(root.join("d"));
        fake.stats.insert(root.join("f"), fake_stat());
        let db = DirSQL::with_ignore_and_fs(root, Vec::new(), Vec::<String>::new(), Arc::new(fake))
            .unwrap();
        db.hold(vec![
            FileEvent::Deleted(root.join("f")),
            FileEvent::Modified(root.join("f")),
            FileEvent::Deleted(root.join("d")),
            FileEvent::Created(root.join("gone")),
            FileEvent::Rescan,
        ]);

        assert_eq!(
            db.take_held(),
            vec![
                FileEvent::Created(root.join("f")),
                FileEvent::Created(root.join("d")),
                FileEvent::Deleted(root.join("gone")),
                FileEvent::Rescan,
            ]
        );
        assert_eq!(held(&db), Vec::new());
    }

    type Reads = Arc<Mutex<Vec<String>>>;

    /// A per-file `items` table over `**/*.txt` whose hook records each file
    /// it reads and answers one row holding the file's content, failing for
    /// content `bad`.
    fn content_table() -> (Table, Reads) {
        let reads: Reads = Arc::default();
        let seen = Arc::clone(&reads);
        let table = Table::try_new(
            "items",
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            move |path| {
                seen.lock().unwrap().push(
                    Path::new(path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
                let text = std::fs::read_to_string(path).unwrap_or_default();
                if text == "bad" {
                    return Err("bad content".into());
                }
                Ok(vec![Row::from_iter([(
                    "name".to_string(),
                    Value::Text(text),
                )])])
            },
        );
        (table, reads)
    }

    fn stored_names(db: &DirSQL) -> Vec<String> {
        let guard = db.inner.db.lock().unwrap();
        let mut stmt = guard.conn().prepare("SELECT name FROM items").unwrap();
        let mut names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        names.sort();
        names
    }

    fn failing_paths(db: &DirSQL) -> Vec<(String, String)> {
        db.inner
            .failed_files
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    fn failing_fixture(failing: &[(&str, &str)]) -> DirSQL {
        let dir = TempDir::new().unwrap();
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![],
            Vec::<String>::new(),
            Arc::new(FakeFs::default()),
        )
        .unwrap();
        for (table, path) in failing {
            db.record_read(table, path, Some("broken"));
        }
        db
    }

    #[test]
    fn a_query_raises_the_first_failing_file_by_table_and_path() {
        let db = failing_fixture(&[("t", "b.txt"), ("t", "a.txt")]);
        match db.query("SELECT 1") {
            Err(DirSqlError::OnFile {
                table,
                path,
                message,
            }) => {
                assert_eq!(
                    (table.as_str(), path.as_str(), message.as_str()),
                    ("t", "a.txt", "broken")
                );
            }
            other => panic!("expected the hook failure, got: {other:?}"),
        }
    }

    #[test]
    fn a_query_succeeds_when_no_file_fails() {
        let db = failing_fixture(&[]);
        assert_eq!(db.query("SELECT 1").unwrap().len(), 1);
    }

    #[test]
    fn a_successful_read_clears_only_its_own_failure() {
        let db = failing_fixture(&[("t", "a.txt"), ("t", "b.txt"), ("u", "a.txt")]);
        db.record_read("t", "a.txt", None);
        assert_eq!(
            failing_paths(&db),
            vec![("t".into(), "b.txt".into()), ("u".into(), "a.txt".into())]
        );
    }

    #[test]
    fn forgetting_failures_under_a_path_keeps_siblings_and_other_tables() {
        let db = failing_fixture(&[
            ("t", "d"),
            ("t", "d/a.txt"),
            ("t", "d/e/b.txt"),
            ("t", "dx/c.txt"),
            ("u", "d/a.txt"),
        ]);
        db.forget_failures_under("t", "d");
        assert_eq!(
            failing_paths(&db),
            vec![
                ("t".into(), "dx/c.txt".into()),
                ("u".into(), "d/a.txt".into())
            ]
        );
    }

    #[test]
    fn handle_upsert_records_its_failure_and_a_later_success_clears_it() {
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("a.txt");
        let fails = Arc::new(AtomicBool::new(true));
        let hook_fails = Arc::clone(&fails);
        let db = DirSQL::with_ignore_and_fs(
            dir.path(),
            vec![Table::try_new(
                "items",
                "CREATE TABLE items (name TEXT)",
                "**/*.txt",
                move |_| {
                    if hook_fails.load(Ordering::SeqCst) {
                        Err("boom".into())
                    } else {
                        Ok(vec![])
                    }
                },
            )],
            Vec::<String>::new(),
            Arc::new(FakeFs::with_stat(abs.clone(), fake_stat())),
        )
        .unwrap();
        db.handle_upsert("items", &abs, "a.txt");
        assert_eq!(failing_paths(&db), vec![("items".into(), "a.txt".into())]);
        fails.store(false, Ordering::SeqCst);
        db.handle_upsert("items", &abs, "a.txt");
        assert_eq!(failing_paths(&db), vec![]);
    }

    #[test]
    #[cfg(unix)]
    fn a_file_that_cannot_be_statted_fails_rather_than_reading_as_gone() {
        let (_dir, root, db, _) = fresh_fixture(&[("f.txt", "one")]);
        let events = db.handle_upsert("items", &root.join("f.txt/a.txt"), "f.txt/a.txt");
        assert!(
            matches!(events.as_slice(), [RowEvent::Error { .. }]),
            "got: {events:?}"
        );
        assert_eq!(
            failing_paths(&db),
            vec![("items".to_string(), "f.txt/a.txt".to_string())]
        );
    }

    #[test]
    fn deleting_a_failing_file_clears_its_failure() {
        let db = failing_fixture(&[("items", "a.txt")]);
        db.handle_delete("items", "a.txt");
        assert_eq!(failing_paths(&db), vec![]);
    }

    fn fresh_fixture(files: &[(&str, &str)]) -> (TempDir, PathBuf, DirSQL, Reads) {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for (rel, text) in files {
            std::fs::write(root.join(rel), text).unwrap();
        }
        let (table, reads) = content_table();
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![table], Vec::<String>::new(), Arc::new(RealFs))
                .unwrap();
        (dir, root, db, reads)
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_query_reads_a_file_written_after_the_build() {
        let (_dir, root, db, _) = fresh_fixture(&[]);
        db.start_applier();
        std::fs::write(root.join("a.txt"), "one").unwrap();
        await_row_names(&db, &["one"]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_query_reads_an_edit_to_a_file_the_build_indexed() {
        let (_dir, root, db, _) = fresh_fixture(&[("a.txt", "one")]);
        db.start_applier();
        std::fs::write(root.join("a.txt"), "two").unwrap();
        await_row_names(&db, &["two"]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_query_drops_the_rows_of_a_deleted_file() {
        let (_dir, root, db, _) = fresh_fixture(&[("a.txt", "one"), ("b.txt", "two")]);
        db.start_applier();
        std::fs::remove_file(root.join("a.txt")).unwrap();
        await_row_names(&db, &["two"]);
    }

    #[test]
    fn a_query_holds_the_index_still() {
        let (_dir, _root, db, _) = fresh_fixture(&[("a.txt", "one")]);
        let held = db.refresh().unwrap();
        assert!(db.inner.fresh.try_lock().is_err());
        drop(held);
        assert!(db.inner.fresh.try_lock().is_ok());
    }

    #[test]
    fn lock_index_marks_the_index_locked() {
        let (_dir, _root, db, _) = fresh_fixture(&[]);
        db.lock_index().unwrap();
        assert!(db.inner.fresh.lock().unwrap().locked);
    }

    #[test]
    fn unlock_index_lets_a_query_run_at_once() {
        let (_dir, _root, db, _) = fresh_fixture(&[("a.txt", "one")]);
        db.lock_index().unwrap();
        db.unlock_index().unwrap();
        assert!(!db.inner.fresh.lock().unwrap().locked);
        assert_eq!(row_names(&db), vec!["one"]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn query_ordered_and_query_each_read_fresh_rows_too() {
        let (_dir, root, db, _) = fresh_fixture(&[]);
        db.start_applier();
        std::fs::write(root.join("a.txt"), "one").unwrap();
        await_row_names(&db, &["one"]);
        assert_eq!(
            db.query_ordered("SELECT name FROM items")
                .unwrap()
                .rows
                .len(),
            1
        );
        std::fs::write(root.join("b.txt"), "two").unwrap();
        await_row_names(&db, &["one", "two"]);
        let count = db
            .query_each("SELECT name FROM items", |_| 0, |n, _| *n += 1)
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_file_whose_hook_fails_after_the_build_keeps_its_rows_and_fails_the_query() {
        let (_dir, root, db, _) = fresh_fixture(&[("a.txt", "one"), ("b.txt", "two")]);
        db.start_applier();
        std::fs::write(root.join("a.txt"), "uno").unwrap();
        std::fs::write(root.join("b.txt"), "bad").unwrap();
        for _ in 0..1000 {
            if db.query("SELECT name FROM items").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        match db.query("SELECT name FROM items") {
            Err(DirSqlError::OnFile {
                table,
                path,
                message,
            }) => {
                assert_eq!((table.as_str(), message.as_str()), ("items", "bad content"));
                assert!(path.ends_with("b.txt"), "got: {path}");
            }
            other => panic!("expected the hook failure, got: {other:?}"),
        }
        assert_eq!(stored_names(&db), vec!["two", "uno"]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_per_table_hook_reruns_when_one_of_its_files_changes_and_when_one_goes() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("a.txt"), "x").unwrap();
        let (items, calls) = recording_per_table("items", "*.txt");
        let db =
            DirSQL::with_ignore_and_fs(&root, vec![items], Vec::<String>::new(), Arc::new(RealFs))
                .unwrap();
        db.start_applier();
        std::fs::write(root.join("b.txt"), "x").unwrap();
        await_row_names(&db, &["a.txt", "b.txt"]);
        assert_eq!(
            calls.lock().unwrap().last().unwrap(),
            &vec![root.join("a.txt"), root.join("b.txt")]
        );
        std::fs::remove_file(root.join("a.txt")).unwrap();
        await_row_names(&db, &["b.txt"]);
        assert_eq!(
            calls.lock().unwrap().last().unwrap(),
            &vec![root.join("b.txt")]
        );
    }

    fn link_at(path: &str, target: &str) -> Link {
        Link {
            path: PathBuf::from(path),
            target: PathBuf::from(target),
        }
    }

    #[test]
    fn restate_moves_a_change_below_the_target_below_the_link() {
        let link = link_at("/ws/link", "/ws/real");
        for (event, restated) in [
            (
                FileEvent::Modified("/ws/real/x.txt".into()),
                FileEvent::Modified("/ws/link/x.txt".into()),
            ),
            (
                FileEvent::Created("/ws/real/a/x.txt".into()),
                FileEvent::Created("/ws/link/a/x.txt".into()),
            ),
            (
                FileEvent::Deleted("/ws/real/x.txt".into()),
                FileEvent::Deleted("/ws/link/x.txt".into()),
            ),
        ] {
            assert_eq!(link.restate(&event), Some(restated));
        }
    }

    #[test]
    fn restate_puts_the_target_itself_at_the_link() {
        let link = link_at("/ws/link", "/ws/real");
        assert_eq!(
            link.restate(&FileEvent::Deleted("/ws/real".into())),
            Some(FileEvent::Deleted("/ws/link".into()))
        );
    }

    #[test]
    fn restate_puts_a_directory_above_the_target_appearing_or_leaving_at_the_link() {
        let link = link_at("/ws/link", "/ws/a/real");
        assert_eq!(
            link.restate(&FileEvent::Deleted("/ws/a".into())),
            Some(FileEvent::Deleted("/ws/link".into()))
        );
        assert_eq!(
            link.restate(&FileEvent::Created("/ws/a".into())),
            Some(FileEvent::Created("/ws/link".into()))
        );
        assert_eq!(link.restate(&FileEvent::Modified("/ws/a".into())), None);
    }

    #[test]
    fn restate_leaves_an_unrelated_change_and_a_rescan_alone() {
        let link = link_at("/ws/link", "/ws/real");
        assert_eq!(
            link.restate(&FileEvent::Modified("/ws/other/x.txt".into())),
            None
        );
        assert_eq!(
            link.restate(&FileEvent::Modified("/ws/realm/x.txt".into())),
            None
        );
        assert_eq!(link.restate(&FileEvent::Rescan), None);
    }

    /// A db over `/ws` that knows `/ws/link` resolves to `/ws/real`, holding
    /// `real/x.txt` and `link/x.txt`.
    fn linked_db() -> DirSQL {
        let root = PathBuf::from("/ws");
        let fake = FakeFs::default()
            .with_canonical_root(&root, "/ws")
            .with_canonical_root("/ws/link", "/ws/real")
            .with_subtree("/ws/link", vec![("/ws/link/x.txt".into(), "items".into())])
            .with_subtree("/ws/real", vec![("/ws/real/x.txt".into(), "items".into())]);
        let db = subtree_db(fake, &root, Vec::new());
        db.process_file_event(FileEvent::Created("/ws/real".into()));
        db.process_file_event(FileEvent::Created("/ws/link".into()));
        db
    }

    fn known_links(db: &DirSQL) -> Vec<Link> {
        db.inner.links.lock().unwrap().clone()
    }

    #[test]
    fn a_created_directory_that_resolves_elsewhere_is_known_as_a_link() {
        let db = linked_db();
        assert_eq!(known_links(&db), vec![link_at("/ws/link", "/ws/real")]);
    }

    #[test]
    fn a_change_at_the_target_reaches_the_rows_under_the_link() {
        let db = linked_db();

        let events = db.process_file_event(FileEvent::Deleted("/ws/real/x.txt".into()));

        assert_eq!(
            deleted_paths(&events),
            vec!["link/x.txt".to_string(), "real/x.txt".to_string()]
        );
    }

    #[test]
    fn deleting_the_link_forgets_it() {
        let db = linked_db();
        db.process_file_event(FileEvent::Deleted("/ws/link".into()));
        assert!(known_links(&db).is_empty());
    }

    #[test]
    fn a_deletion_reached_only_through_a_link_keeps_the_link_known() {
        let db = linked_db();
        db.process_file_event(FileEvent::Deleted("/ws/real".into()));
        assert_eq!(known_links(&db), vec![link_at("/ws/link", "/ws/real")]);
    }

    #[test]
    fn a_directory_below_a_known_link_is_not_a_link_of_its_own() {
        let db = linked_db();
        db.register_link("/ws/link".into());
        assert!(db.below_a_link(Path::new("/ws/link/sub")));
        assert!(!db.below_a_link(Path::new("/ws/link")));
        assert!(!db.below_a_link(Path::new("/ws/linked")));
        assert_eq!(known_links(&db).len(), 1, "re-registering replaces");
    }

    #[test]
    fn a_created_directory_below_a_known_link_is_not_a_link_of_its_own() {
        let root = PathBuf::from("/ws");
        let fake = FakeFs::default()
            .with_canonical_root(&root, "/ws")
            .with_canonical_root("/ws/link", "/ws/real")
            .with_canonical_root("/ws/link/sub", "/ws/real/sub")
            .with_subtree("/ws/link", Vec::new())
            .with_subtree("/ws/link/sub", Vec::new());
        let db = subtree_db(fake, &root, Vec::new());
        db.process_file_event(FileEvent::Created("/ws/link".into()));

        db.process_file_event(FileEvent::Created("/ws/link/sub".into()));

        assert_eq!(known_links(&db), vec![link_at("/ws/link", "/ws/real")]);
    }

    #[test]
    fn a_directory_that_resolves_to_itself_is_no_link() {
        let db = linked_db();
        db.register_link("/ws/real".into());
        assert_eq!(known_links(&db), vec![link_at("/ws/link", "/ws/real")]);
    }

    #[test]
    fn the_startup_walk_reports_the_links_it_entered_under_the_watch_root() {
        let dir = TempDir::new().unwrap();
        let resolved = resolved_over(dir.path(), vec![txt_table()], false);
        let link = dir.path().join("link");
        let prepared = DirSQL::prepare_resolved_with(resolved, &|_, _, _, _, on_link| {
            on_link(&link);
            Vec::new()
        })
        .unwrap();
        assert_eq!(prepared.links, vec![link.clone()]);

        let fake = FakeFs::default()
            .with_canonical_root(dir.path(), "/ws")
            .with_canonical_root("/ws/link", "/ws/real");
        let db =
            DirSQL::finish_build_with_fs(prepared, Arc::new(fake), Progress::indexing()).unwrap();

        assert_eq!(known_links(&db), vec![link_at("/ws/link", "/ws/real")]);
    }
}

#[cfg(test)]
mod command_rows_tests {
    use super::*;

    fn rows(payload: &str) -> Vec<Row> {
        let parsed = infer::parse_rows(payload).unwrap();
        let columns: Vec<String> = parsed
            .first()
            .map(|row| row.0.iter().map(|(key, _)| format!("\"{key}\"")).collect())
            .unwrap_or_else(|| vec!["x".to_string()]);
        let db = Db::new().unwrap();
        db.create_table("t", &format!("CREATE TABLE t ({})", columns.join(", ")))
            .unwrap();
        db.shape_rows("t", parsed, false).unwrap().to_maps()
    }

    #[test]
    fn converts_one_row_object_per_line() {
        let rows = rows("{\"id\":\"a\",\"n\":1}\n{\"id\":\"b\",\"n\":2}");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], Value::Text("a".into()));
        assert_eq!(rows[0]["n"], Value::Integer(1));
        assert_eq!(rows[1]["id"], Value::Text("b".into()));
        assert_eq!(rows[1]["n"], Value::Integer(2));
    }

    #[test]
    fn converts_empty_output_to_no_rows() {
        assert_eq!(rows(""), Vec::<Row>::new());
    }

    #[test]
    fn maps_every_json_value_type_including_nested_to_text_json() {
        let rows = rows(
            r#"{"nul":null,"t":true,"f":false,"i":42,"r":1.5,"s":"hi","arr":[1,2],"obj":{"k":"v"}}"#,
        );
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
        let rows = rows(r#"{"big":10000000000000000000}"#);
        assert!(matches!(rows[0]["big"], Value::Real(_)));
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
