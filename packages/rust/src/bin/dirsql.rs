//! `dirsql` CLI binary. Two modes:
//! - No subcommand: HTTP server documented in `docs/guide/cli.md`.
//! - `init`: starter `.dirsql.toml` generation; see `docs/guide/init.md`.
//!
//! Only compiled with `--features cli`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use dirsql::cli::{
    AppState, ServerConfig,
    init::InitOptions,
    native_config::{InterpretHelper, build_dirsql},
    serve_with_state,
};
use dirsql::{DirSQL, Row, Table};

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
    let server_config = ServerConfig::bind(cli.host.clone(), cli.port);

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

    if is_native_config(&resolved) {
        return load_native_state(&resolved);
    }

    match DirSQL::from_config_path(&resolved) {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!("failed to load config: {err}")),
    }
}

/// Native-language config support: `--config X.{py,js,mjs,cjs}` delegates
/// to `dirsql interpret <X>` (spawned via PATH) for `extract` execution.
/// The binary still owns SQL, HTTP, and the file watcher.
fn is_native_config(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("py") | Some("js") | Some("mjs") | Some("cjs")
    )
}

fn load_native_state(config_path: &Path) -> AppState {
    let (helper, config) = match spawn_interpret_helper(config_path) {
        Ok(x) => x,
        Err(err) => return AppState::Unavailable(err),
    };
    match build_dirsql(helper, config) {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!(
            "failed to build DirSQL from {}: {err}",
            config_path.display()
        )),
    }
}

/// Spawn `dirsql interpret <config_path>` via PATH and hand the child
/// off to [`InterpretHelper::from_child`]. Lives in the CLI binary
/// (rather than the lib's `cli::native_config` module) because the
/// `Command::new("dirsql")` plumbing is only meaningfully exercised
/// end-to-end via the `dirsql --config X.{py,js,mjs,cjs}` integration
/// path — there's no useful in-process unit test for it.
fn spawn_interpret_helper(
    config_path: &Path,
) -> Result<
    (
        std::sync::Arc<InterpretHelper>,
        dirsql::cli::native_config::NativeConfig,
    ),
    String,
> {
    use std::process::{Command, Stdio};
    let child = Command::new("dirsql")
        .arg("interpret")
        .arg(config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            format!(
                "failed to spawn `dirsql interpret`: {e}. \
                 Native-language configs require a launcher that implements `interpret` \
                 on PATH (install dirsql via pip/uv or npm/npx)."
            )
        })?;
    InterpretHelper::from_child(child)
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
    fn is_native_config_matches_script_extensions() {
        for ext in ["py", "js", "mjs", "cjs"] {
            let path = format!("cfg.{ext}");
            assert!(
                is_native_config(Path::new(&path)),
                "expected .{ext} to be treated as a native config"
            );
        }
    }

    #[test]
    fn is_native_config_rejects_other_extensions_and_casing() {
        // `.toml` is the built-in format; bare/uppercase extensions are not
        // delegated to the `interpret` helper.
        for name in ["cfg.toml", "cfg.txt", "cfg", "cfg.PY", "cfg.JS"] {
            assert!(
                !is_native_config(Path::new(name)),
                "expected {name} not to be treated as a native config"
            );
        }
    }
}
