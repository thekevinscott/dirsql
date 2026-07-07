//! `dirsql init` — write a fixed starter `.dirsql.toml`. See
//! `docs/reference/cli.md` for the user-facing contract.
//!
//! `init` does not inspect the target directory at all: it writes the same
//! single `files` table that [zero-config mode](../../bin/dirsql.rs)
//! already serves, so a user always has something loadable to hand-edit.
//! No LLM, no network, no filesystem walk.

use std::path::PathBuf;

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

/// The fixed starter config `init` writes -- byte-for-byte the same
/// `[[table]]` block the zero-config default (`default_files_table` in
/// `src/bin/dirsql.rs`) uses.
const STARTER_TOML: &str = "[[table]]\nddl  = \"CREATE TABLE files (_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, _size INTEGER, _mtime INTEGER, _ctime INTEGER)\"\nglob = \"**/*\"\n";

pub fn run(opts: InitOptions) -> Result<(), InitError> {
    if !opts.force && opts.output.exists() {
        return Err(InitError::AlreadyExists { path: opts.output });
    }

    std::fs::write(&opts.output, STARTER_TOML.as_bytes()).map_err(|source| InitError::Write {
        path: opts.output.clone(),
        source,
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

    #[test]
    fn starter_toml_declares_the_default_files_table_over_recursive_glob() {
        assert!(STARTER_TOML.starts_with("[[table]]\nddl  = \"CREATE TABLE files ("));
        assert!(STARTER_TOML.contains("_path TEXT"));
        assert!(STARTER_TOML.contains("_basename TEXT"));
        assert!(STARTER_TOML.contains("_dir TEXT"));
        assert!(STARTER_TOML.contains("_ext TEXT"));
        assert!(STARTER_TOML.contains("_size INTEGER"));
        assert!(STARTER_TOML.contains("_mtime INTEGER"));
        assert!(STARTER_TOML.contains("_ctime INTEGER"));
        assert!(STARTER_TOML.contains("glob = \"**/*\""));
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

    #[test]
    fn run_writes_the_starter_toml() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join(".dirsql.toml");
        let opts = InitOptions {
            output: output.clone(),
            force: false,
        };
        run(opts).unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), STARTER_TOML);
    }

    #[test]
    fn run_with_force_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join(".dirsql.toml");
        std::fs::write(&output, "# old\n").unwrap();
        let opts = InitOptions {
            output: output.clone(),
            force: true,
        };
        run(opts).unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), STARTER_TOML);
    }
}
