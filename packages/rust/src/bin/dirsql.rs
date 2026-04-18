//! `dirsql` CLI binary. Server implementation (POST /query, GET /events SSE,
//! graceful shutdown) lands in a follow-up PR tracked by #105. This stub
//! exists so downstream distribution channels (cargo-dist, npm launcher, pypi
//! launcher) have a stable bin target to package, and so the `cli` feature
//! flag is exercised end-to-end (compile + run) before the heavier server
//! dependencies land.

use clap::Parser;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "dirsql",
    version,
    about = "Ephemeral SQL index over a local directory (CLI).",
    long_about = "Runs an HTTP server exposing a SQL view of a directory. \
                  Server implementation is tracked in #105; this stub \
                  currently prints --version / --help and exits."
)]
struct Cli {}

fn main() -> ExitCode {
    let _ = Cli::parse();
    eprintln!("dirsql: server not yet implemented (tracking #105)");
    ExitCode::from(2)
}
