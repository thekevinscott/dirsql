//! `dirsql` CLI binary. Three modes:
//! - No subcommand: HTTP server documented in `docs/reference/cli.md`.
//! - `query`: one-shot query over the same pipeline the server uses; see
//!   `docs/reference/cli.md`.
//! - `init`: writes a fixed starter `.dirsql.toml`; see `docs/reference/cli.md`.
//!
//! Only compiled with `--features cli`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use dirsql::cli::{
    AppState, PostQuery, PreQuery, ServerConfig, execute::execute_query, init::InitOptions,
    serve_with_state,
};
use dirsql::{DirSQL, Extension, Row, Table};

#[derive(Debug, Parser)]
#[command(
    name = "dirsql",
    version,
    about = "Ephemeral SQL index over a local directory, exposed over HTTP.",
    long_about = "Runs an HTTP server that exposes a SQL view of a local \
                  directory. Tables are defined by a `.dirsql.toml` config \
                  file; with no config, a default `files` table over every \
                  file in the directory is served. With the `init` \
                  subcommand, writes that same default `files` table as a \
                  starter `.dirsql.toml` — no target-directory inspection, \
                  no network, deterministic."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the config file (default: `./.dirsql.toml`). The index is
    /// rooted at the invocation directory (cwd), not this file's location
    /// (#540). When the file does not exist, a default `files` table is
    /// served. Used by server mode and by the `query` subcommand.
    #[arg(short = 'c', long, default_value = "./.dirsql.toml", global = true)]
    config: PathBuf,

    /// Bind address. Used when no subcommand is given.
    #[arg(long, default_value = "localhost")]
    host: String,

    /// TCP port to bind. Used when no subcommand is given.
    #[arg(long, default_value_t = 7117)]
    port: u16,

    /// Load a SQLite extension by literal path, overriding a TOML config's
    /// `[[dirsql.extension]]` entries. Repeatable. Format: `<path>` or
    /// `<path>::<entrypoint>`.
    ///
    /// Intended for the language launcher (pip/npm), not end users: the
    /// launcher resolves config extensions — including bare **package names**,
    /// which need an interpreter this compiled binary lacks — and passes the
    /// resolved literal paths here. When any are present, the TOML
    /// config's own extension entries are not loaded (the launcher already
    /// merged and resolved them). Used by server mode and by the `query`
    /// subcommand.
    #[arg(long = "extension", global = true)]
    extension: Vec<String>,

    /// Keep the SQLite index on disk between runs so a restart only re-parses
    /// files that actually changed. Bare `--persist` caches at the default
    /// location (`<root>/.dirsql/cache.db`); `--persist <path>` caches there.
    /// Off by default (ephemeral index). Used by server mode and `query`.
    #[arg(long, num_args = 0..=1, global = true)]
    persist: Option<Option<PathBuf>>,
}

impl Cli {
    /// Apply the `--persist [PATH]` flag to a builder. Absent → no change;
    /// bare `--persist` → persist at the default location; `--persist <path>`
    /// → persist at `<path>`.
    fn apply_persist(&self, mut builder: dirsql::DirSQLBuilder) -> dirsql::DirSQLBuilder {
        if let Some(path) = &self.persist {
            builder = builder.persist(path.as_ref());
        }
        builder
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write the fixed starter `.dirsql.toml` — the same default `files`
    /// table zero-config mode serves. No target-directory inspection.
    Init(InitArgs),

    /// Run one SQL query against the indexed directory, print the result
    /// rows as JSON on stdout, and exit. No server, no watch. Shares the
    /// server's query pipeline, so config discovery, hooks, the query
    /// timeout, the read-only rule, and error classification are identical
    /// to `POST /query`.
    Query(QueryArgs),
}

#[derive(Debug, Args)]
struct QueryArgs {
    /// The SQL to run (a single read-only statement).
    sql: String,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Directory the default `--output` path is resolved against (default:
    /// current directory). The written config's content does not depend on
    /// this directory's contents.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Where to write the generated config (default: `<root>/.dirsql.toml`).
    #[arg(long)]
    output: Option<PathBuf>,

    /// Overwrite the output file if it already exists.
    #[arg(long)]
    force: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut cli = Cli::parse();

    match cli.command.take() {
        Some(Command::Init(args)) => run_init(args),
        Some(Command::Query(args)) => run_query(&cli, args).await,
        None => run_server(cli).await,
    }
}

/// One-shot `dirsql query`: build the index exactly as server mode would
/// (same `load_state` / hook loading), run the SQL through the shared
/// [`execute_query`] pipeline, print the result JSON on stdout, and exit.
/// Any [`QueryFailure`](dirsql::cli::execute::QueryFailure) prints its
/// message — the same string the HTTP `{"error": …}` body carries — to
/// stderr with a non-zero exit.
async fn run_query(cli: &Cli, args: QueryArgs) -> ExitCode {
    let state = load_state(cli);
    let pre_query = load_pre_query(cli);
    let post_query = load_post_query(cli);
    // Same default the server binds with; the pipeline enforces it.
    let timeout = ServerConfig::default().query_timeout;

    match execute_query(
        &state,
        query_body(&args.sql),
        timeout,
        pre_query.as_ref(),
        post_query.as_ref(),
    )
    .await
    {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!("dirsql query: {}", failure.message());
            ExitCode::from(1)
        }
    }
}

/// Synthesize the exact `POST /query` body for a positional SQL argument,
/// so the shared pipeline's intake validation and `pre-query` hook see
/// byte-for-byte what an HTTP client would send.
fn query_body(sql: &str) -> String {
    serde_json::json!({ "sql": sql }).to_string()
}

fn run_init(args: InitArgs) -> ExitCode {
    let root = match args.root {
        Some(p) => p,
        None => match std::env::current_dir() {
            Ok(p) => p,
            Err(err) => {
                eprintln!("dirsql init: failed to read current directory: {err}");
                return ExitCode::from(1);
            }
        },
    };
    let output = args.output.unwrap_or_else(|| root.join(".dirsql.toml"));

    let opts = InitOptions {
        output,
        force: args.force,
    };

    match dirsql::cli::init::run(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("dirsql init: {err}");
            ExitCode::from(1)
        }
    }
}

async fn run_server(cli: Cli) -> ExitCode {
    let state = load_state(&cli);
    let mut server_config = ServerConfig::bind(cli.host.clone(), cli.port);
    if let Some(pre_query) = load_pre_query(&cli) {
        server_config = server_config.with_pre_query(pre_query);
    }
    if let Some(post_query) = load_post_query(&cli) {
        server_config = server_config.with_post_query(post_query);
    }

    let host = cli.host.clone();
    let handle = match serve_with_state(server_config, state).await {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("dirsql: failed to bind: {err}");
            return ExitCode::from(1);
        }
    };

    // Echo back the user-facing hostname (not the resolved IP SocketAddr).
    println!("Running at {host}:{}", handle.local_addr().port());

    if let Err(err) = wait_for_shutdown().await {
        eprintln!("dirsql: signal handler error: {err}");
    }

    if let Err(err) = handle.shutdown().await {
        eprintln!("dirsql: shutdown error: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn load_state(cli: &Cli) -> AppState {
    let config_path = &cli.config;
    if !config_path.exists() {
        // No config: serve a default `files` table so dirsql is queryable
        // out of the box. A config file, when present, fully overrules this.
        return load_default_state(cli, config_path);
    }

    // Canonicalize so config-relative paths (extension libraries, hook
    // working directories) resolve against an absolute parent — `notify` and
    // the hook subprocesses misbehave with relative paths like `./`. The
    // index root itself is the invocation cwd (#540), not derived from here.
    let resolved = match config_path.canonicalize() {
        Ok(p) => p,
        Err(err) => {
            return AppState::Unavailable(format!(
                "failed to resolve {}: {err}",
                config_path.display()
            ));
        }
    };

    // Launcher-resolved extensions (`--extension`) override the TOML config's
    // own `[[dirsql.extension]]` entries: the launcher has already merged and
    // resolved them (including package names the compiled binary can't
    // resolve), so suppress the config's extension loading and supply the
    // resolved literal paths instead.
    let mut builder = DirSQL::builder().config(&resolved);
    if !cli.extension.is_empty() {
        builder = builder
            .extensions(parse_extension_specs(&cli.extension))
            .suppress_config_extensions(true);
    }
    builder = cli.apply_persist(builder);
    match builder.build() {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!("failed to load config: {err}")),
    }
}

/// Parse `--extension` specs (`<path>` or `<path>::<entrypoint>`) into
/// [`Extension`]s. Splitting on the first `::` keeps a path that itself
/// contains `::` unambiguous only after the entrypoint boundary — entrypoints
/// are C identifiers, so the first `::` is the boundary.
fn parse_extension_specs(specs: &[String]) -> Vec<Extension> {
    specs
        .iter()
        .map(|spec| match spec.split_once("::") {
            Some((path, entrypoint)) => Extension {
                path: PathBuf::from(path),
                entrypoint: Some(entrypoint.to_string()),
            },
            None => Extension {
                path: PathBuf::from(spec),
                entrypoint: None,
            },
        })
        .collect()
}

/// Extract the server-wide `pre-query` hook from the config, if any.
///
/// Returns `None` when the config is absent, unresolvable, unparsable, or
/// declares no `pre-query` — the server then parses `POST /query` bodies as
/// `{"sql": …}` (the degraded / zero-config paths never get a hook). The
/// command's working directory is the config file's parent, mirroring the
/// `on-file` contract. Config resolution mirrors [`load_state`]: a config that
/// fails here also fails there (leaving the server degraded), so the hook is
/// simply skipped.
fn load_pre_query(cli: &Cli) -> Option<PreQuery> {
    let config_path = &cli.config;
    if !config_path.exists() {
        return None;
    }
    let resolved = config_path.canonicalize().ok()?;
    let config = dirsql::config::load_config(&resolved).ok()?;
    let command = config.pre_query?;
    let config_dir = resolved.parent()?.to_path_buf();
    let mut pre_query = PreQuery::new(command, config_dir);
    if let Some(timeout) = config.hook_timeout {
        pre_query = pre_query.with_timeout(timeout);
    }
    Some(pre_query)
}

/// Extract the server-wide `post-query` hook from the config, if any.
///
/// Returns `None` when the config is absent, unresolvable, unparsable, or
/// declares no `post-query` — the server then returns `POST /query` result rows
/// as-is (the degraded / zero-config paths never get a hook). The command's
/// working directory is the config file's parent, mirroring [`load_pre_query`].
fn load_post_query(cli: &Cli) -> Option<PostQuery> {
    let config_path = &cli.config;
    if !config_path.exists() {
        return None;
    }
    let resolved = config_path.canonicalize().ok()?;
    let config = dirsql::config::load_config(&resolved).ok()?;
    let command = config.post_query?;
    let config_dir = resolved.parent()?.to_path_buf();
    let mut post_query = PostQuery::new(command, config_dir);
    if let Some(timeout) = config.hook_timeout {
        post_query = post_query.with_timeout(timeout);
    }
    Some(post_query)
}

/// Zero-config fallback. When no `.dirsql.toml` is found, dirsql indexes the
/// directory that would have held the config with a single default `files`
/// table — one row per file, columns drawn entirely from filesystem facts —
/// so `SELECT * FROM files` works immediately. A config file, when present,
/// fully overrules this default.
fn load_default_state(cli: &Cli, config_path: &Path) -> AppState {
    let dir = config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // Canonicalize for the same reason `load_state` does: `notify` misbehaves
    // when watching relative paths.
    let root = match dir.canonicalize() {
        Ok(p) => p,
        Err(err) => {
            return AppState::Unavailable(format!("failed to resolve {}: {err}", dir.display()));
        }
    };

    let builder = cli.apply_persist(DirSQL::builder().root(root).table(default_files_table()));
    match builder.build() {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!("failed to build default index: {err}")),
    }
}

/// The default `files` table used in zero-config mode, parsed from the same
/// [`dirsql::cli::DEFAULT_CONFIG_TOML`] asset `dirsql init` writes verbatim,
/// so the two can never drift apart.
fn default_files_table() -> Table {
    let config = dirsql::config::load_config_str(dirsql::cli::DEFAULT_CONFIG_TOML)
        .expect("DEFAULT_CONFIG_TOML must be valid dirsql config TOML");
    let table_config = &config.tables[0];
    Table::new(
        table_config.ddl.clone(),
        table_config.glob.clone(),
        |_path| vec![Row::new()],
    )
}

#[cfg(unix)]
async fn wait_for_shutdown() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut term = signal(SignalKind::terminate())?;
    let mut intr = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = term.recv() => {}
        _ = intr.recv() => {}
    }
    Ok(())
}

#[cfg(not(unix))]
async fn wait_for_shutdown() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_body_wraps_sql_in_the_http_request_shape() {
        // The subcommand must feed the pipeline the exact body a curl'd
        // `POST /query` carries, so intake validation stays single-sourced.
        assert_eq!(query_body("SELECT 1"), r#"{"sql":"SELECT 1"}"#);
    }

    #[test]
    fn query_body_escapes_sql_as_json() {
        // SQL containing quotes/newlines must arrive as valid JSON, not be
        // spliced raw into the body.
        assert_eq!(
            query_body("SELECT \"a\"\nFROM t"),
            r#"{"sql":"SELECT \"a\"\nFROM t"}"#
        );
    }

    #[test]
    fn query_body_preserves_blank_sql_for_the_shared_rejection() {
        // Blank SQL is NOT rejected here: it flows to the pipeline's shared
        // empty-rejection so both surfaces emit the identical message.
        assert_eq!(query_body("   "), r#"{"sql":"   "}"#);
    }

    #[test]
    fn persist_flag_absent_is_none() {
        let cli = Cli::parse_from(["dirsql"]);
        assert_eq!(cli.persist, None);
    }

    #[test]
    fn persist_flag_bare_enables_default_location() {
        // Bare `--persist` (no value) → `Some(None)`: persist at the default
        // `<root>/.dirsql/cache.db`, no override path.
        let cli = Cli::parse_from(["dirsql", "--persist"]);
        assert_eq!(cli.persist, Some(None));
    }

    #[test]
    fn persist_flag_with_path_carries_the_value() {
        let cli = Cli::parse_from(["dirsql", "--persist", "/var/cache/x.db"]);
        assert_eq!(cli.persist, Some(Some(PathBuf::from("/var/cache/x.db"))));
    }

    #[test]
    fn persist_flag_is_global_on_the_query_subcommand() {
        // `--persist` is global, so it attaches to `query` too; the flag sits
        // after the positional SQL to avoid the num_args(0..=1) greedy grab.
        let cli = Cli::parse_from(["dirsql", "query", "SELECT 1", "--persist"]);
        assert_eq!(cli.persist, Some(None));
    }

    #[test]
    fn parse_extension_specs_handles_bare_path_and_entrypoint() {
        let specs = vec![
            "/abs/vec0.so".to_string(),
            "/abs/spellfix.so::sqlite3_spellfix_init".to_string(),
        ];
        let exts = parse_extension_specs(&specs);
        assert_eq!(exts.len(), 2);
        assert_eq!(exts[0].path, PathBuf::from("/abs/vec0.so"));
        assert!(exts[0].entrypoint.is_none());
        assert_eq!(exts[1].path, PathBuf::from("/abs/spellfix.so"));
        assert_eq!(exts[1].entrypoint.as_deref(), Some("sqlite3_spellfix_init"));
    }

    #[test]
    fn parse_extension_specs_splits_on_first_double_colon() {
        let specs = vec!["/a.so::init::extra".to_string()];
        let exts = parse_extension_specs(&specs);
        assert_eq!(exts[0].path, PathBuf::from("/a.so"));
        assert_eq!(exts[0].entrypoint.as_deref(), Some("init::extra"));
    }

    #[test]
    fn default_files_table_declares_filesystem_fact_columns_over_recursive_glob() {
        let table = default_files_table();
        assert_eq!(table.glob, "**/*");
        assert!(table.ddl.starts_with("CREATE TABLE files ("));
        for col in ["path", "basename", "dir", "ext", "size", "mtime", "ctime"] {
            assert!(
                table.ddl.contains(col),
                "default files DDL must declare {col}, got: {}",
                table.ddl
            );
        }
    }
}
