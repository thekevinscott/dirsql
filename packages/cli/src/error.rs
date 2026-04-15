//! Error types for the CLI.

use thiserror::Error;

/// Error for query-path failures returned to HTTP clients.
#[derive(Debug, Error)]
pub enum QueryError {
    #[error("engine error: {0}")]
    Engine(String),
    #[error("unsupported value: {0}")]
    Unsupported(String),
}

/// Top-level CLI error (startup/binding/config loading).
#[derive(Debug, Error)]
pub enum CliError {
    #[error("failed to load config at {path}: {message}")]
    Config { path: String, message: String },

    #[error("failed to bind to {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },

    #[error("engine initialization failed: {0}")]
    Engine(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_error_display_contains_message() {
        let e = QueryError::Engine("boom".into());
        assert!(e.to_string().contains("boom"));
        let e = QueryError::Unsupported("blob".into());
        assert!(e.to_string().contains("blob"));
    }

    #[test]
    fn cli_error_display() {
        let e = CliError::Config {
            path: "/tmp/x".into(),
            message: "missing".into(),
        };
        let s = e.to_string();
        assert!(s.contains("config"));
        assert!(s.contains("/tmp/x"));
        assert!(s.contains("missing"));
        let e = CliError::Engine("nope".into());
        assert!(e.to_string().contains("nope"));
    }
}
