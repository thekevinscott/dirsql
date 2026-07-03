//! `dirsql` CLI binary. Two modes:
//! - No subcommand: HTTP server documented in `docs/guide/cli.md`.
//! - `init`: starter `.dirsql.toml` generation; see `docs/guide/init.md`.
//!
//! Only compiled with `--features cli`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use dirsql::cli::{
    AppState, PostQuery, PreQuery, ServerConfig, init::InitOptions, serve_with_state,
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
                  subcommand, generates a starter `.dirsql.toml` by running \
                  `claude` over the target directory."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the config file (default: `./.dirsql.toml`). The index is
    /// rooted at the directory containing this file. When the file does
    /// not exist, a default `files` table is served. Used when no
    /// subcommand is given.
    #[arg(long, default_value = "./.dirsql.toml")]
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
    /// which need an interpreter this compiled binary lacks (see #227) — and
    /// passes the resolved literal paths here. When any are present, the TOML
    /// config's own extension entries are not loaded (the launcher already
    /// merged and resolved them).
    #[arg(long = "extension")]
    extension: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate a starter `.dirsql.toml` by running `claude` over the
    /// target directory.
    Init(InitArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Directory to scan (default: current directory).
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
        None => run_server(cli).await,
    }
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
        root,
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

    // Await ctrl-c / SIGTERM; then drain.
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
        return load_default_state(config_path);
    }

    // Canonicalize so the root (derived from the config's parent) is
    // absolute — `notify` has surprising behavior when watching relative
    // paths like `./`.
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
    // resolved them (including package names the compiled binary can't resolve;
    // #227), so build from the config but suppress its extension loading and
    // supply the resolved literal paths instead.
    let build = if cli.extension.is_empty() {
        DirSQL::from_config_path(&resolved)
    } else {
        DirSQL::builder()
            .config(&resolved)
            .extensions(parse_extension_specs(&cli.extension))
            .suppress_config_extensions(true)
            .build()
    };
    match build {
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
fn load_default_state(config_path: &Path) -> AppState {
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

    match DirSQL::new(root, vec![default_files_table()]) {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!("failed to build default index: {err}")),
    }
}

/// The default `files` table used in zero-config mode: glob `**/*` matches
/// every file under the root at any depth (no ignores), and each row is built
/// purely from the auto-injected filesystem-fact columns.
fn default_files_table() -> Table {
    Table::new(
        "CREATE TABLE files (\
         _path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, \
         _size INTEGER, _mtime INTEGER, _ctime INTEGER)",
        "**/*",
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
        // The zero-config fallback table is pure data: a fixed DDL naming only
        // the auto-injected filesystem-fact columns and a `**/*` glob that
        // matches every file at any depth. The extract closure is never
        // invoked here, so this stays a pure unit test.
        let table = default_files_table();
        assert_eq!(table.glob, "**/*");
        assert!(table.ddl.starts_with("CREATE TABLE files ("));
        for col in [
            "_path",
            "_basename",
            "_dir",
            "_ext",
            "_size",
            "_mtime",
            "_ctime",
        ] {
            assert!(
                table.ddl.contains(col),
                "default files DDL must declare {col}, got: {}",
                table.ddl
            );
        }
    }
}
