//! `dirsql init` — write a fixed starter `.dirsql.toml`. See
//! `docs/reference/cli.md` for the user-facing contract.
//!
//! `init` does not inspect the target directory at all: it writes
//! [`super::DEFAULT_CONFIG_TOML`] verbatim -- the escalation scaffold (a named
//! `[[table]]` with glob, DDL, and a real `on-file` hook) that the
//! `--include-default` launcher path also seeds from -- so a user always has
//! a loadable, working config to hand-edit, and the two surfaces can never
//! drift apart. No LLM, no network, no filesystem walk.

use std::path::PathBuf;

use super::DEFAULT_CONFIG_TOML;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("{}: already exists; pass --force to overwrite", path.display())]
    AlreadyExists { path: PathBuf },

    #[error("failed to write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Path the generated config is written to. Resolved by the caller
    /// (default: `<root>/.dirsql.toml`).
    pub output: PathBuf,
    /// Overwrite `output` if it already exists.
    pub force: bool,
}

pub fn run(opts: InitOptions) -> Result<(), InitError> {
    if !opts.force && opts.output.exists() {
        return Err(InitError::AlreadyExists { path: opts.output });
    }

    std::fs::write(&opts.output, DEFAULT_CONFIG_TOML.as_bytes()).map_err(|source| {
        InitError::Write {
            path: opts.output.clone(),
            source,
        }
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_exists_error_mentions_force() {
        let err = InitError::AlreadyExists {
            path: PathBuf::from("/tmp/foo.toml"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("already exists"));
        assert!(msg.contains("--force"));
    }

    #[test]
    fn write_error_display_names_the_path_and_source() {
        let err = InitError::Write {
            path: PathBuf::from("/tmp/out.toml"),
            source: std::io::Error::other("disk full"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/tmp/out.toml"), "got: {msg}");
        assert!(msg.contains("disk full"), "got: {msg}");
    }

    /// The temp dir itself serves as the already-existing output path.
    #[test]
    fn run_bails_when_output_exists_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let opts = InitOptions {
            output: dir.path().to_path_buf(),
            force: false,
        };
        let err = run(opts).unwrap_err();
        assert!(
            matches!(err, InitError::AlreadyExists { .. }),
            "got: {err:?}"
        );
    }

    // The write success + `--force` paths are covered black-box in
    // `tests/init_integration.rs` (unit-lint isolation keeps effectful
    // `std::fs` out of here).
}
