use clap::Parser;
use dirsql_cli::args::Args;
use dirsql_cli::engine::{DirSQLEngine, QueryEngine};
use dirsql_cli::error::CliError;
use dirsql_cli::server::build_app;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dirsql: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), CliError> {
    let resolved = args.resolve()?;

    // Pre-validate the config exists to produce a clean error message.
    if !resolved.config.exists() {
        return Err(CliError::Config {
            path: resolved.config.display().to_string(),
            message: "config file not found (expected .dirsql.toml)".into(),
        });
    }

    let engine = build_engine(&resolved.dir, &resolved.config)?;

    let addr = format!("{}:{}", resolved.host, resolved.port);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(CliError::Io)?;

    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| CliError::Bind {
                addr: addr.clone(),
                source: e,
            })?;
        let app = build_app(engine);
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| CliError::Engine(e.to_string()))?;
        Ok::<(), CliError>(())
    })
}

fn build_engine(dir: &Path, config: &Path) -> Result<Arc<dyn QueryEngine>, CliError> {
    let engine = DirSQLEngine::from_config_path(dir, Some(config))
        .map_err(|e| CliError::Engine(e.to_string()))?;
    Ok(Arc::new(engine))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
