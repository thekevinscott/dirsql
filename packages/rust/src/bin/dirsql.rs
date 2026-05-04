//! `dirsql` CLI binary. The default invocation starts the HTTP server
//! documented in `docs/guide/cli.md`. The `init` subcommand generates a
//! starter `.dirsql.toml` (see `docs/guide/init.md`). Only compiled with
//! `--features cli`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dirsql::DirSQL;
use dirsql::cli::init::{InitArgs, run as run_init};
use dirsql::cli::{AppState, ServerConfig, serve_with_state};

#[derive(Debug, Parser)]
#[command(
    name = "dirsql",
    version,
    about = "Ephemeral SQL index over a local directory, exposed over HTTP.",
    long_about = "Runs an HTTP server that exposes a SQL view of the directory \
                  described by `.dirsql.toml`. Use `dirsql init` to generate a \
                  starter config. See the CLI guide for endpoints and SSE schema."
)]
struct Cli {
    /// Path to the config file (default: `./.dirsql.toml`). The index is
    /// rooted at the directory containing this file.
    #[arg(long, default_value = "./.dirsql.toml", global = true)]
    config: PathBuf,

    /// Bind address.
    #[arg(long, default_value = "localhost", global = true)]
    host: String,

    /// TCP port to bind.
    #[arg(long, default_value_t = 7117, global = true)]
    port: u16,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate a starter `.dirsql.toml` (heuristic by default; pass
    /// `--infer` for the LLM-assisted variant).
    Init(InitArgs),
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(Command::Init(args)) = cli.command {
        let code = run_init(args);
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }

    serve(cli).await
}

async fn serve(cli: Cli) -> ExitCode {
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
        return AppState::Unavailable(format!("config not found at {}", config_path.display()));
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

    match DirSQL::from_config_path(&resolved) {
        Ok(db) => AppState::Ready(db),
        Err(err) => AppState::Unavailable(format!("failed to load config: {err}")),
    }
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
