//! The `dirsql` CLI: argument parsing and command dispatch behind
//! [`run_cli`]. Query is the default; the server is a subcommand:
//! - No subcommand + SQL (`dirsql "<sql>"`): one-shot query, the default
//!   behavior. Identical to `dirsql query "<sql>"`.
//! - `query`: explicit synonym for the default one-shot query; see
//!   `docs/reference/cli.md`.
//! - `server`: the HTTP server documented in `docs/reference/cli.md`.
//! - `init`: writes a fixed starter `.dirsql.toml`; see `docs/reference/cli.md`.
//! - No subcommand and no SQL: a usage error pointing at `dirsql server`.
//!
//! Only compiled with `--features cli`.
//!
//! The `dirsql` binary is a shim over [`run_cli`], so `cargo install dirsql
//! --features cli` and every other entry path run the same code.

use std::path::PathBuf;

use super::{
    AppState, PostQuery, PreQuery, ServerConfig, execute::execute_query, init::InitOptions,
    serve_with_state,
};
use crate::{DirSQL, Extension, Row, Table};
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "dirsql",
    version,
    about = "Query a local directory as SQL. `dirsql \"<sql>\"` runs one query; \
             `dirsql server` starts the HTTP server.",
    long_about = "Runs one SQL query over a local directory and prints the \
                  result rows as JSON. `dirsql \"SELECT * FROM './'\"` is the \
                  default; `dirsql query \"<sql>\"` is an explicit synonym. \
                  Tables are defined by a `.dirsql.toml` config file passed \
                  with `-c`; with no `-c` there are no named tables and \
                  filesystem queries go through path-tables \
                  (`SELECT * FROM './'`) — a `./.dirsql.toml` on disk is NOT \
                  auto-loaded, pass it explicitly. The `server` subcommand \
                  runs the long-lived HTTP server instead. Config flags are \
                  subcommand-local: for `query`/`server` pass them AFTER the \
                  subcommand (`dirsql query <sql> -c <cfg>`); a flag before a \
                  subcommand is a hard error. The `init` subcommand writes a \
                  starter `.dirsql.toml` defining a `files` table — no \
                  target-directory inspection, no network, deterministic.",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// The SQL to run in the default (query) mode: `dirsql "<sql>"`. With no
    /// subcommand and no SQL, dirsql prints a usage error pointing at
    /// `dirsql server`. Identical to `dirsql query "<sql>"`.
    sql: Option<String>,

    /// Attach a parser to every path-table in the default-mode query — the
    /// same `--on-file` the `query` subcommand takes (see [`QueryArgs`]).
    #[arg(long = "on-file")]
    on_file: Vec<String>,

    /// Config flags for the default (query) mode. They are subcommand-local,
    /// not global: for the `query`/`server` subcommands the same flags are
    /// passed AFTER the subcommand (`dirsql query <sql> -c <cfg>`); a config
    /// flag placed BEFORE a subcommand is a hard error, never silently
    /// dropped (#609).
    #[command(flatten)]
    common: ConfigArgs,
}

/// The config-layer flags shared by server mode and the `query` subcommand.
/// Flattened into both `Cli` (server) and `QueryArgs` (query) rather than
/// declared `global`, so a repeatable `-c` cannot straddle the subcommand
/// boundary and be silently dropped -- misplacement is a hard clap error
/// (`args_conflicts_with_subcommands`) instead (#609).
#[derive(Debug, Args)]
struct ConfigArgs {
    /// Path to a config file. **Repeatable** (`-c a -c b`): the configs load
    /// and merge in argv order -- their `[[table]]`, `ignore`, and
    /// `[[dirsql.extension]]` entries accumulate, and their `pre-query` /
    /// `post-query` hooks chain FIFO. With none given, no named tables are
    /// defined -- query the filesystem with a path-table (`FROM './'`). A
    /// `./.dirsql.toml` on disk is NOT auto-loaded (#602); pass it explicitly
    /// to use it. A `-c` naming a missing file is an error. The index is rooted at the
    /// invocation directory (cwd), not a config's location (#540). For `query`,
    /// pass this AFTER the subcommand (`dirsql query <sql> -c <cfg>`).
    #[arg(short = 'c', long)]
    config: Vec<PathBuf>,

    /// Internal (launcher-only): seed the resolved config set with the shipped
    /// starter `records` table *before* the `-c` configs, so an explicit `-c`
    /// composes with it instead of standing alone. `--include-default -c
    /// <plugin>` yields that table **plus** the plugin's tables — the additive
    /// composition the plugin launcher (#529) injects for the no-user-`-c`
    /// case (#604). This is an explicit opt-in, not the implicit no-`-c`
    /// fallback (which was retired in #636). Hidden from `--help`: it is
    /// internal plumbing for the launcher, not a documented public flag.
    #[arg(long = "include-default", hide = true)]
    include_default: bool,

    /// Load a SQLite extension by literal path, overriding a TOML config's
    /// `[[dirsql.extension]]` entries. Repeatable. Format: `<path>` or
    /// `<path>::<entrypoint>`.
    ///
    /// Intended for the language launcher (pip/npm), not end users: the
    /// launcher resolves config extensions — including bare **package names**,
    /// which need an interpreter this compiled binary lacks — and passes the
    /// resolved literal paths here. When any are present, the TOML
    /// config's own extension entries are not loaded (the launcher already
    /// merged and resolved them).
    #[arg(long = "extension")]
    extension: Vec<String>,

    /// Keep the SQLite index on disk between runs so a restart only re-parses
    /// files that actually changed. Bare `--persist` caches at the default
    /// location (`<root>/.dirsql/cache.db`); `--persist <path>` caches there.
    /// Off by default (ephemeral index).
    #[arg(long, num_args = 0..=1)]
    persist: Option<Option<PathBuf>>,

    /// Scan files a `.gitignore` would hide. Path-tables respect `.gitignore`
    /// files by default (hierarchically, like fd/ripgrep); this flag restores
    /// the full walk. The built-in skips (`node_modules`/`.git`) and any
    /// configured `ignore` patterns still apply.
    #[arg(long = "no-ignore")]
    no_ignore: bool,
}

impl ConfigArgs {
    /// Apply the index-shaping flags to a builder: `--persist [PATH]` (absent
    /// → no change; bare `--persist` → the default location; `--persist
    /// <path>` → that path) and `--no-ignore`.
    fn apply_index_flags(&self, mut builder: crate::DirSQLBuilder) -> crate::DirSQLBuilder {
        if let Some(path) = &self.persist {
            builder = builder.persist(path.as_ref());
        }
        builder.no_ignore(self.no_ignore)
    }

    /// The config paths passed via `-c`/`--config`. Empty when none were given
    /// -- no named tables are defined, and there is no implicit
    /// `./.dirsql.toml` discovery (#602).
    fn config_paths(&self) -> Vec<PathBuf> {
        self.config.clone()
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write the fixed starter `.dirsql.toml` — a `files` table over every
    /// file in the directory. The output does not auto-load; pass it with
    /// `dirsql query <sql> -c ./.dirsql.toml`. No target-directory
    /// inspection.
    Init(InitArgs),

    /// Run one SQL query against the indexed directory, print the result
    /// rows as JSON on stdout, and exit. No server, no watch. This is the
    /// explicit synonym for the default `dirsql "<sql>"`. Shares the
    /// server's query pipeline, so config loading, hooks, the query
    /// timeout, the read-only rule, and error classification are identical
    /// to `POST /query`. Config flags follow the SQL: `dirsql query <sql> -c <cfg>`.
    Query(QueryArgs),

    /// Start the long-lived HTTP server exposing a SQL view of the directory
    /// over `POST /query` and `GET /events`. Config flags follow the
    /// subcommand: `dirsql server -c <cfg>`. Runs until `SIGINT` / `SIGTERM`.
    Server(ServerArgs),
}

#[derive(Debug, Args)]
struct ServerArgs {
    /// Bind address.
    #[arg(long, default_value = "localhost")]
    host: String,

    /// TCP port to bind.
    #[arg(long, default_value_t = 7117)]
    port: u16,

    #[command(flatten)]
    common: ConfigArgs,
}

#[derive(Debug, Args)]
struct QueryArgs {
    /// The SQL to run (a single read-only statement).
    sql: String,

    /// Attach a parser to every path-table in the query. The command follows
    /// the `on-file` hook contract (`docs/reference/hooks.md`): argv splitting,
    /// `{path}`/`{root}` placeholders, a JSON array of row objects on stdout,
    /// per-file failure isolation, and the hook timeout. With it set, a
    /// path-table's rows and schema come from the parser instead of the stat
    /// columns. One `--on-file` max; for multiple tables use a config file.
    #[arg(long = "on-file")]
    on_file: Vec<String>,

    #[command(flatten)]
    common: ConfigArgs,
}

/// Reduce the repeatable `--on-file` occurrences to at most one parser command.
///
/// `clap` collects repeats into a `Vec` so the error can name config files
/// (its default "cannot be used multiple times" cannot). Empty → no parser;
/// exactly one → that command; more than one → an error pointing at config
/// files, where per-table parsers belong. A whitespace-only command is rejected
/// up front (the hook contract forbids an empty command).
fn resolve_on_file(occurrences: &[String]) -> std::result::Result<Option<String>, String> {
    match occurrences {
        [] => Ok(None),
        [command] if command.trim().is_empty() => {
            Err("--on-file needs a non-empty command".to_string())
        }
        [command] => Ok(Some(command.clone())),
        _ => Err(
            "--on-file may be given at most once; it applies to every path-table in the \
             query. For per-table parsers, define tables in a config file with an \
             `on-file` key and pass it with -c."
                .to_string(),
        ),
    }
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

/// Run the CLI over an explicit `argv` and return its exit code.
///
/// The single entry point every packaging path shares: the `dirsql` binary is
/// a shim over this, and the language launchers reach it through their
/// bindings. Per the bin-shim contract it **always returns** — it never
/// terminates the host process, so an embedder stays in control of its own
/// exit. `argv[0]` is the program name, as `std::env::args()` yields it.
///
/// Exit codes are ordinary status codes only (never 130/143): a `SIGINT` to
/// `dirsql server` shuts down gracefully and returns 0, which is what the
/// binary has always done. Signals are handled *within* the run by
/// [`wait_for_shutdown`]; nothing is re-raised on the way out.
///
/// Clap's own `parse()` exits the process on `--help`, `--version` and usage
/// errors, so this parses fallibly and renders the result itself — same text,
/// same streams, same codes, minus the exit.
pub fn run_cli(argv: Vec<String>) -> i32 {
    let mut cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(err) => {
            let code = err.exit_code();
            // Writes help/version to stdout and usage errors to stderr,
            // exactly as clap's own `parse()` does before exiting.
            let _ = err.print();
            return code;
        }
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("dirsql: failed to start the async runtime: {err}");
            return 1;
        }
    };

    let code = runtime.block_on(async {
        match cli.command.take() {
            Some(Command::Init(args)) => run_init(args),
            Some(Command::Query(args)) => run_query(args).await,
            Some(Command::Server(args)) => run_server(args).await,
            None => run_default(cli).await,
        }
    });
    i32::from(code)
}

/// The default (no-subcommand) behavior: run the positional SQL as a one-shot
/// query, exactly as `dirsql query "<sql>"` does. With no SQL there is nothing
/// to run and no mode selected, so print a usage error pointing at
/// `dirsql server` — silently starting the server here would re-invert the
/// default this design deliberately fixed (#662).
async fn run_default(cli: Cli) -> u8 {
    match cli.sql {
        Some(sql) => {
            run_query(QueryArgs {
                sql,
                on_file: cli.on_file,
                common: cli.common,
            })
            .await
        }
        None => {
            eprintln!(
                "dirsql: no query given. Run a query with `dirsql \"SELECT * FROM './'\"`, \
                 or start the HTTP server with `dirsql server`. See `dirsql --help`."
            );
            2
        }
    }
}

/// One-shot `dirsql query`: build the index exactly as server mode would
/// (same `load_state` / hook loading), run the SQL through the shared
/// [`execute_query`] pipeline, print the result JSON on stdout, and exit.
/// Any [`QueryFailure`](super::execute::QueryFailure) prints its
/// message — the same string the HTTP `{"error": …}` body carries — to
/// stderr with a non-zero exit.
async fn run_query(args: QueryArgs) -> u8 {
    let parser = match resolve_on_file(&args.on_file) {
        Ok(parser) => parser,
        Err(message) => {
            eprintln!("dirsql query: {message}");
            return 1;
        }
    };
    let state = load_state(&args.common, parser);
    let skipped = report_scan_failures(&state);
    let pre_query = load_pre_queries(&args.common);
    let post_query = load_post_queries(&args.common);
    // Same default the server binds with; the pipeline enforces it.
    let timeout = ServerConfig::default().query_timeout;

    match execute_query(
        &state,
        query_body(&args.sql),
        timeout,
        &pre_query,
        &post_query,
    )
    .await
    {
        Ok(value) => {
            println!("{value}");
            // The query ran and its rows are on stdout, so this is not a
            // failure -- but the index behind them is missing files, and a
            // caller piping into `jq` under `set -e` has no other way to find
            // that out.
            if skipped { PARTIAL_SCAN_EXIT } else { 0 }
        }
        Err(failure) => {
            eprintln!("dirsql query: {}", failure.message());
            1
        }
    }
}

/// Exit code for "the scan completed, but some files were skipped" -- distinct
/// from `1` so a script can tell a partial index from a run that failed.
/// Follows rsync's `23` ("partial transfer due to error").
const PARTIAL_SCAN_EXIT: u8 = 23;

/// How many skipped files to name before collapsing the rest into a count. A
/// directory of unreadable files should not bury the terminal.
const MAX_REPORTED_FAILURES: usize = 10;

/// Print the scan's skipped files to stderr, capped, and report whether there
/// were any. stdout is left for the query result alone.
fn report_scan_failures(state: &AppState) -> bool {
    let AppState::Ready(db) = state else {
        return false;
    };
    let failures = db.scan_failures();
    if failures.is_empty() {
        return false;
    }
    for failure in failures.iter().take(MAX_REPORTED_FAILURES) {
        eprintln!("dirsql: skipping `{}`: {}", failure.path, failure.message);
    }
    if let Some(rest) = failures
        .len()
        .checked_sub(MAX_REPORTED_FAILURES)
        .filter(|n| *n > 0)
    {
        eprintln!("dirsql: ... and {rest} more");
    }
    true
}

/// Synthesize the exact `POST /query` body for a positional SQL argument,
/// so the shared pipeline's intake validation and `pre-query` hook see
/// byte-for-byte what an HTTP client would send.
fn query_body(sql: &str) -> String {
    serde_json::json!({ "sql": sql }).to_string()
}

fn run_init(args: InitArgs) -> u8 {
    let root = match args.root {
        Some(p) => p,
        None => match std::env::current_dir() {
            Ok(p) => p,
            Err(err) => {
                eprintln!("dirsql init: failed to read current directory: {err}");
                return 1;
            }
        },
    };
    let output = args.output.unwrap_or_else(|| root.join(".dirsql.toml"));

    let opts = InitOptions {
        output,
        force: args.force,
    };

    match super::init::run(opts) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("dirsql init: {err}");
            1
        }
    }
}

async fn run_server(args: ServerArgs) -> u8 {
    // The server has no `--on-file`: clap rejects it as an unknown flag before
    // reaching here. Path-tables served over HTTP keep their stat columns.
    let state = load_state(&args.common, None);
    let mut server_config = ServerConfig::bind(args.host.clone(), args.port);
    for pre_query in load_pre_queries(&args.common) {
        server_config = server_config.with_pre_query(pre_query);
    }
    for post_query in load_post_queries(&args.common) {
        server_config = server_config.with_post_query(post_query);
    }

    let host = args.host.clone();
    let handle = match serve_with_state(server_config, state).await {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("dirsql: failed to bind: {err}");
            return 1;
        }
    };

    // Echo back the user-facing hostname (not the resolved IP SocketAddr).
    println!("Running at {host}:{}", handle.local_addr().port());

    if let Err(err) = wait_for_shutdown().await {
        eprintln!("dirsql: signal handler error: {err}");
    }

    if let Err(err) = handle.shutdown().await {
        eprintln!("dirsql: shutdown error: {err}");
        return 1;
    }
    0
}

fn load_state(cfg: &ConfigArgs, path_table_parser: Option<String>) -> AppState {
    // Neither a `-c` nor the launcher's `--include-default` -> index the
    // invocation directory with no named tables. A `./.dirsql.toml` on disk is
    // NOT consulted (#602); pass it explicitly with `-c` to use it.
    if cfg.config.is_empty() && !cfg.include_default {
        return load_configless_state(cfg, path_table_parser);
    }

    let mut builder = DirSQL::builder();
    // `--include-default` seeds the shipped starter `records` table before the
    // `-c` configs, so an explicit config composes with it instead of standing
    // alone (#604). Programmatic tables sort before config tables in
    // `resolve`, giving `[starter] ++ [-c]`; a starter-vs-config `records`
    // collision hits the existing dedup in `compile_matcher`. With no `-c` at
    // all the flag still applies, yielding just the starter table.
    if cfg.include_default {
        builder = builder.table(default_records_table());
    }
    for config_path in &cfg.config {
        // Canonicalize so config-relative paths (extension libraries, hook
        // working directories) resolve against an absolute parent — `notify`
        // and the hook subprocesses misbehave with relative paths like `./`.
        // The index root itself is the invocation cwd (#540), not derived here.
        let resolved = match config_path.canonicalize() {
            Ok(p) => p,
            Err(err) => {
                return AppState::Unavailable(format!(
                    "failed to resolve {}: {err}",
                    config_path.display()
                ));
            }
        };
        builder = builder.config(resolved);
    }

    // Launcher-resolved extensions (`--extension`) override the configs' own
    // `[[dirsql.extension]]` entries: the launcher has already merged and
    // resolved them (including package names the compiled binary can't
    // resolve), so suppress config extension loading and supply the resolved
    // literal paths instead.
    if !cfg.extension.is_empty() {
        builder = builder
            .extensions(parse_extension_specs(&cfg.extension))
            .suppress_config_extensions(true);
    }
    builder = cfg.apply_index_flags(builder);
    // `--on-file` touches path-tables only: config `[[table]]` definitions keep
    // their own `on-file` hooks; a path-table named in the query gets this
    // parser regardless of any `-c`.
    if let Some(command) = path_table_parser {
        builder = builder.path_table_parser(command);
    }
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

/// Collect the `pre-query` hooks declared across the configs, in argv order,
/// so the server chains them FIFO (#546/#547).
///
/// Each config contributes at most one `pre-query`; a config that is absent,
/// unresolvable, unparsable, or declares none is skipped (its load failure
/// degrades the index in [`load_state`], so the hook is simply omitted here).
/// Each hook's working directory is its own config file's parent, mirroring
/// the `on-file` contract, and it carries that config's `hook-timeout`.
fn load_pre_queries(cfg: &ConfigArgs) -> Vec<PreQuery> {
    let mut hooks = Vec::new();
    for config_path in &cfg.config_paths() {
        if !config_path.exists() {
            continue;
        }
        let Ok(resolved) = config_path.canonicalize() else {
            continue;
        };
        let Ok(config) = crate::config::load_config(&resolved) else {
            continue;
        };
        let Some(command) = config.pre_query else {
            continue;
        };
        let Some(parent) = resolved.parent() else {
            continue;
        };
        let mut pre_query = PreQuery::new(command, parent.to_path_buf());
        if let Some(timeout) = config.hook_timeout {
            pre_query = pre_query.with_timeout(timeout);
        }
        hooks.push(pre_query);
    }
    hooks
}

/// Collect the `post-query` hooks declared across the configs, in argv order,
/// so the server chains them FIFO. Mirrors [`load_pre_queries`]: one hook per
/// config, skipped when absent/unloadable, each running from its own config's
/// parent under its own `hook-timeout`.
fn load_post_queries(cfg: &ConfigArgs) -> Vec<PostQuery> {
    let mut hooks = Vec::new();
    for config_path in &cfg.config_paths() {
        if !config_path.exists() {
            continue;
        }
        let Ok(resolved) = config_path.canonicalize() else {
            continue;
        };
        let Ok(config) = crate::config::load_config(&resolved) else {
            continue;
        };
        let Some(command) = config.post_query else {
            continue;
        };
        let Some(parent) = resolved.parent() else {
            continue;
        };
        let mut post_query = PostQuery::new(command, parent.to_path_buf());
        if let Some(timeout) = config.hook_timeout {
            post_query = post_query.with_timeout(timeout);
        }
        hooks.push(post_query);
    }
    hooks
}

/// With no `-c`, dirsql indexes the invocation directory but defines no named
/// tables: filesystem queries go through path-tables (`SELECT * FROM './'`),
/// and a `files` query fails with a hint pointing at that form (#636). A
/// `./.dirsql.toml` in the cwd is not consulted (#602).
fn load_configless_state(cfg: &ConfigArgs, path_table_parser: Option<String>) -> AppState {
    // Canonicalize for the same reason `load_state` does: `notify` misbehaves
    // when watching relative paths.
    let root = match PathBuf::from(".").canonicalize() {
        Ok(p) => p,
        Err(err) => {
            return AppState::Unavailable(format!("failed to resolve current directory: {err}"));
        }
    };

    // `--extension` applies here too (#772). Without this the flag was
    // silently ignored whenever no `-c` was given: `dirsql query "SELECT
    // vec_version()" --extension <path>` reported `no such function` rather
    // than loading anything, and `--extension /nonexistent.so` exited 0. A
    // path-table query over an extension-provided function is a legitimate
    // configless use, and a bad path must still fail loudly.
    //
    // Applied unconditionally: `extensions([])` assigns the builder's own
    // default, and `apply_index_flags` never touches the field, so an
    // `is_empty()` guard would only add a branch no unit test can reach --
    // `load_configless_state` needs a real cwd and index (#233).
    let mut builder = cfg
        .apply_index_flags(DirSQL::builder().root(root))
        .extensions(parse_extension_specs(&cfg.extension));
    if let Some(command) = path_table_parser {
        builder = builder.path_table_parser(command);
    }
    match builder.build() {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!("failed to build the index: {err}")),
    }
}

/// The shipped starter `records` table, parsed from the [`crate::DEFAULT_CONFIG_TOML`]
/// asset `dirsql init` writes. Used only by the explicit `--include-default`
/// compose path (#604), which seeds it as a programmatic table *before* the
/// `-c` configs. There is no implicit no-`-c` fallback (#636).
fn default_records_table() -> Table {
    let config = crate::config::load_config_str(crate::DEFAULT_CONFIG_TOML)
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

    /// The `ConfigArgs` parsed from a `query` subcommand invocation (#609:
    /// config flags are subcommand-local, so they live on the Query variant).
    fn query_common(argv: &[&str]) -> ConfigArgs {
        match Cli::parse_from(argv).command {
            Some(Command::Query(args)) => args.common,
            other => panic!("expected a query subcommand, got {other:?}"),
        }
    }

    #[test]
    fn config_paths_is_empty_without_a_config_flag() {
        // No `-c` -> no config paths at all, with no implicit `./.dirsql.toml`
        // discovery (#602).
        let cli = Cli::parse_from(["dirsql", "SELECT 1"]);
        assert!(cli.common.config_paths().is_empty());
    }

    #[test]
    fn config_paths_returns_exactly_the_passed_paths() {
        // Default query mode (no subcommand): `-c` accumulates at the top
        // level alongside the positional SQL; the paths are exactly those, in
        // argv order.
        let cli = Cli::parse_from(["dirsql", "SELECT 1", "-c", "a.toml", "-c", "b.toml"]);
        assert_eq!(
            cli.common.config_paths(),
            vec![PathBuf::from("a.toml"), PathBuf::from("b.toml")]
        );
    }

    #[test]
    fn bare_sql_parses_as_the_default_query_with_no_subcommand() {
        // #662: `dirsql "<sql>"` (no subcommand) carries the SQL as the
        // top-level positional and selects no command -> the default query.
        let cli = Cli::parse_from(["dirsql", "SELECT * FROM './'"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.sql.as_deref(), Some("SELECT * FROM './'"));
    }

    #[test]
    fn bare_sql_carries_config_flags_for_the_default_query() {
        // #662: the default query mode accepts the same `-c` the `query`
        // subcommand does, after the SQL.
        let cli = Cli::parse_from(["dirsql", "SELECT 1", "-c", "a.toml"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.sql.as_deref(), Some("SELECT 1"));
        assert_eq!(cli.common.config_paths(), vec![PathBuf::from("a.toml")]);
    }

    #[test]
    fn bare_sql_carries_on_file_for_the_default_query() {
        // #662: `--on-file` works in the default mode exactly as under `query`.
        let cli = Cli::parse_from(["dirsql", "SELECT 1", "--on-file", "cat {path}"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.on_file, vec!["cat {path}".to_string()]);
    }

    #[test]
    fn no_subcommand_and_no_sql_selects_neither() {
        // #662: bare `dirsql` with nothing else parses cleanly (no command, no
        // SQL); `run_default` turns that into the usage error at runtime.
        let cli = Cli::parse_from(["dirsql"]);
        assert!(cli.command.is_none());
        assert!(cli.sql.is_none());
    }

    #[test]
    fn server_subcommand_parses_host_and_port() {
        // #662: the server moved behind `dirsql server`; `--host`/`--port` are
        // now server-local flags.
        match Cli::parse_from(["dirsql", "server", "--host", "0.0.0.0", "--port", "9000"]).command {
            Some(Command::Server(args)) => {
                assert_eq!(args.host, "0.0.0.0");
                assert_eq!(args.port, 9000);
            }
            other => panic!("expected a server subcommand, got {other:?}"),
        }
    }

    #[test]
    fn server_subcommand_defaults_host_and_port() {
        match Cli::parse_from(["dirsql", "server"]).command {
            Some(Command::Server(args)) => {
                assert_eq!(args.host, "localhost");
                assert_eq!(args.port, 7117);
            }
            other => panic!("expected a server subcommand, got {other:?}"),
        }
    }

    #[test]
    fn server_subcommand_carries_config_flags() {
        // Config flags are subcommand-local: `dirsql server -c a -c b`.
        match Cli::parse_from(["dirsql", "server", "-c", "a.toml", "-c", "b.toml"]).command {
            Some(Command::Server(args)) => assert_eq!(
                args.common.config_paths(),
                vec![PathBuf::from("a.toml"), PathBuf::from("b.toml")]
            ),
            other => panic!("expected a server subcommand, got {other:?}"),
        }
    }

    #[test]
    fn host_and_port_are_not_top_level_flags() {
        // #662: `--host`/`--port` moved under `server`; at the top level they
        // are unknown flags now.
        assert!(Cli::try_parse_from(["dirsql", "--host", "0.0.0.0"]).is_err());
        assert!(Cli::try_parse_from(["dirsql", "--port", "9000"]).is_err());
    }

    #[test]
    fn config_flags_parse_after_the_query_subcommand() {
        // #609: config flags are subcommand-local. `dirsql query <sql> -c a -c b`
        // accumulates both on the Query variant, in argv order.
        assert_eq!(
            query_common(&[
                "dirsql", "query", "SELECT 1", "-c", "a.toml", "-c", "b.toml"
            ])
            .config_paths(),
            vec![PathBuf::from("a.toml"), PathBuf::from("b.toml")]
        );
    }

    #[test]
    fn config_flag_before_a_subcommand_is_a_hard_error() {
        // #609: a `-c` BEFORE the subcommand conflicts with it (never silently
        // dropped or straddled). `args_conflicts_with_subcommands` rejects it.
        let result = Cli::try_parse_from(["dirsql", "-c", "a.toml", "query", "SELECT 1"]);
        assert!(
            result.is_err(),
            "a config flag before the subcommand must be rejected, got {result:?}"
        );
    }

    #[test]
    fn include_default_defaults_false_without_the_flag() {
        // Absent -> false: `-c` keeps its replacement semantics unless the
        // launcher explicitly opts the baked-in default back in (#604).
        let cli = Cli::parse_from(["dirsql"]);
        assert!(!cli.common.include_default);
    }

    #[test]
    fn include_default_flag_sets_true() {
        let cli = Cli::parse_from(["dirsql", "--include-default"]);
        assert!(cli.common.include_default);
    }

    #[test]
    fn include_default_parses_after_the_query_subcommand() {
        // Subcommand-local (#609): the launcher injects it AFTER `query`
        // alongside `-c <plugin>`.
        assert!(
            query_common(&["dirsql", "query", "SELECT 1", "--include-default"]).include_default
        );
    }

    #[test]
    fn persist_flag_absent_is_none() {
        let cli = Cli::parse_from(["dirsql"]);
        assert_eq!(cli.common.persist, None);
    }

    #[test]
    fn persist_flag_bare_enables_default_location() {
        // Bare `--persist` (no value) → `Some(None)`: persist at the default
        // `<root>/.dirsql/cache.db`, no override path.
        let cli = Cli::parse_from(["dirsql", "--persist"]);
        assert_eq!(cli.common.persist, Some(None));
    }

    #[test]
    fn persist_flag_with_path_carries_the_value() {
        let cli = Cli::parse_from(["dirsql", "--persist", "/var/cache/x.db"]);
        assert_eq!(
            cli.common.persist,
            Some(Some(PathBuf::from("/var/cache/x.db")))
        );
    }

    #[test]
    fn no_ignore_flag_defaults_false() {
        let cli = Cli::parse_from(["dirsql"]);
        assert!(!cli.common.no_ignore);
    }

    #[test]
    fn no_ignore_flag_sets_true_in_the_default_query_mode() {
        let cli = Cli::parse_from(["dirsql", "SELECT 1", "--no-ignore"]);
        assert!(cli.common.no_ignore);
    }

    #[test]
    fn no_ignore_flag_parses_after_the_query_subcommand() {
        assert!(query_common(&["dirsql", "query", "SELECT 1", "--no-ignore"]).no_ignore);
    }

    #[test]
    fn persist_flag_parses_after_the_query_subcommand() {
        // Subcommand-local (#609); the flag sits after the positional SQL to
        // avoid the num_args(0..=1) greedy grab.
        assert_eq!(
            query_common(&["dirsql", "query", "SELECT 1", "--persist"]).persist,
            Some(None)
        );
    }

    /// The `on_file` occurrences parsed from a `query` invocation.
    fn query_on_file(argv: &[&str]) -> Vec<String> {
        match Cli::parse_from(argv).command {
            Some(Command::Query(args)) => args.on_file,
            other => panic!("expected a query subcommand, got {other:?}"),
        }
    }

    #[test]
    fn on_file_absent_leaves_no_occurrences() {
        assert!(query_on_file(&["dirsql", "query", "SELECT 1"]).is_empty());
    }

    #[test]
    fn on_file_parses_after_the_query_subcommand() {
        assert_eq!(
            query_on_file(&["dirsql", "query", "SELECT 1", "--on-file", "cat {path}"]),
            vec!["cat {path}".to_string()]
        );
    }

    #[test]
    fn on_file_collects_every_repeat_for_the_arity_check() {
        // clap collects repeats; `resolve_on_file` turns >1 into the pointed
        // error rather than clap's generic "cannot be used multiple times".
        assert_eq!(
            query_on_file(&[
                "dirsql",
                "query",
                "SELECT 1",
                "--on-file",
                "a",
                "--on-file",
                "b"
            ]),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn on_file_is_rejected_before_a_query_subcommand() {
        // Like the other subcommand-local flags, `--on-file` ahead of the
        // subcommand is not a server flag: clap rejects it.
        assert!(Cli::try_parse_from(["dirsql", "--on-file", "cat", "query", "SELECT 1"]).is_err());
    }

    #[test]
    fn resolve_on_file_is_none_without_the_flag() {
        assert_eq!(resolve_on_file(&[]), Ok(None));
    }

    #[test]
    fn resolve_on_file_returns_the_single_command() {
        assert_eq!(
            resolve_on_file(&["cat {path}".to_string()]),
            Ok(Some("cat {path}".to_string()))
        );
    }

    #[test]
    fn resolve_on_file_rejects_a_blank_command() {
        let err = resolve_on_file(&["   ".to_string()]).unwrap_err();
        assert!(err.contains("non-empty"), "got: {err}");
    }

    #[test]
    fn resolve_on_file_rejects_a_repeat_and_points_at_config_files() {
        let err = resolve_on_file(&["a".to_string(), "b".to_string()]).unwrap_err();
        assert!(err.contains("at most once"), "got: {err}");
        assert!(
            err.contains("config file"),
            "the error must point at config files, got: {err}"
        );
    }

    #[test]
    fn configless_state_still_carries_extension_specs() {
        // `--extension` with no `-c` took the configless path, which never
        // applied the specs: the flag was silently ignored and a bad path
        // exited 0 (#772). Parsing is the seam a unit test can reach; the
        // load itself needs a real SQLite handle and is covered at the
        // integration tier.
        let cfg = ConfigArgs {
            config: Vec::new(),
            include_default: false,
            extension: vec!["/ext/vec0.so::sqlite3_vec_init".to_string()],
            persist: None,
            no_ignore: false,
        };
        assert!(cfg.config.is_empty(), "this is the configless shape");
        assert!(
            !cfg.extension.is_empty(),
            "and it still carries an extension"
        );
        let specs = parse_extension_specs(&cfg.extension);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, PathBuf::from("/ext/vec0.so"));
        assert_eq!(specs[0].entrypoint.as_deref(), Some("sqlite3_vec_init"));
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
    fn run_cli_returns_zero_for_version_instead_of_exiting() {
        // The contract this slice exists to establish: clap's own `parse()`
        // calls `std::process::exit`, which would take an embedding host down
        // with it. If someone reinstates it, this test kills the test runner
        // rather than failing politely -- which is the point.
        assert_eq!(run_cli(vec!["dirsql".into(), "--version".into()]), 0);
    }

    #[test]
    fn run_cli_returns_zero_for_help() {
        assert_eq!(run_cli(vec!["dirsql".into(), "--help".into()]), 0);
    }

    #[test]
    fn run_cli_returns_clap_usage_code_for_an_unknown_flag() {
        assert_eq!(run_cli(vec!["dirsql".into(), "--nope".into()]), 2);
    }

    #[test]
    fn run_cli_returns_a_usage_code_for_a_config_flag_before_a_subcommand() {
        // `args_conflicts_with_subcommands` makes misplacement a hard error
        // rather than a silently dropped flag (#609); it must surface as a
        // returned code like any other usage error.
        let code = run_cli(vec![
            "dirsql".into(),
            "-c".into(),
            "cfg.toml".into(),
            "query".into(),
            "SELECT 1".into(),
        ]);
        assert_eq!(code, 2);
    }

    #[test]
    fn report_scan_failures_is_false_when_the_index_is_degraded() {
        // The `Ready` arm needs a real scanned directory, so it lives in the
        // integration tier; the degraded arm short-circuits before any I/O.
        let state = AppState::Unavailable("config failed to load".to_string());
        assert!(!report_scan_failures(&state));
    }

    #[test]
    fn default_records_table_is_built_from_the_shipped_starter_config() {
        // `--include-default` seeds this table, so a starter config that
        // stopped parsing would break that flag rather than `dirsql init`.
        let table = default_records_table();
        assert!(table.ddl.contains("records"));
    }
}
