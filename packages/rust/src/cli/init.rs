//! `dirsql init` — generate a starter `.dirsql.toml` by shelling out to
//! the `claude` CLI. See `docs/guide/init.md` for the user-facing
//! contract.
//!
//! The agent's only required responsibility is to print a valid
//! filesystem-fact-only `.dirsql.toml` to stdout. We invoke `claude -p`
//! with the working directory set to the target root, capture stdout,
//! and write it verbatim to the configured output path.
//!
//! Failure modes:
//! - Output already exists and `--force` was not passed: bail before
//!   spawning the agent (no point burning a paid LLM call).
//! - `claude` is not on PATH: surface a descriptive error pointing at
//!   the install docs.
//! - `claude` exits non-zero: surface its stderr; do not write any
//!   partial config.

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("{}: already exists; pass --force to overwrite", path.display())]
    AlreadyExists { path: PathBuf },

    #[error(
        "`claude` not found on PATH; install Claude Code (https://docs.claude.com/en/docs/claude-code/quickstart)"
    )]
    ClaudeNotFound,

    #[error("failed to spawn `claude`: {0}")]
    Spawn(std::io::Error),

    #[error("`claude` exited with {status}\n{stderr}")]
    ClaudeFailed {
        status: std::process::ExitStatus,
        stderr: String,
    },

    #[error("`claude` produced non-UTF8 output")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),

    #[error("failed to write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Directory to scan. Resolved by the caller (default: cwd).
    pub root: PathBuf,
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

    let prompt = build_prompt();
    let output = Command::new("claude")
        .arg("-p")
        .arg(&prompt)
        .current_dir(&opts.root)
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => InitError::ClaudeNotFound,
            _ => InitError::Spawn(err),
        })?;

    if !output.status.success() {
        return Err(InitError::ClaudeFailed {
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let toml = String::from_utf8(output.stdout)?;
    std::fs::write(&opts.output, toml.as_bytes()).map_err(|source| InitError::Write {
        path: opts.output.clone(),
        source,
    })?;

    Ok(())
}

fn build_prompt() -> String {
    PROMPT.to_string()
}

const PROMPT: &str = r#"You are running inside a directory. Your job is to produce a `.dirsql.toml` config file that defines SQL tables over that directory's filesystem.

Inspect the directory structure (files and subdirectories). Then produce a `.dirsql.toml` with one or more `[[table]]` blocks.

Each `[[table]]` block has:
- `ddl`: a SQLite CREATE TABLE statement.
- `glob`: a glob pattern matching files relative to the directory root.

Each row is one matched file. Columns come from these sources ONLY:
- Glob path captures: `{name}` segments in the glob become columns named `name`.
- Stat virtuals (reserved column names): `_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`.

Do NOT include columns sourced from file content (JSON keys, CSV headers, frontmatter, etc.). Content parsing is not configured in `.dirsql.toml`.

Output ONLY the TOML, with no surrounding prose, no markdown fences, no explanation.

Example for a flat directory of mixed files:
[[table]]
ddl  = "CREATE TABLE files (_path TEXT, _ext TEXT, _size INTEGER)"
glob = "*"

Example with path captures:
[[table]]
ddl  = "CREATE TABLE photos (month TEXT, _basename TEXT, _mtime INTEGER)"
glob = "{month}/*.jpg"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_mentions_filesystem_fact_constraints() {
        let p = build_prompt();
        assert!(p.contains("[[table]]"));
        assert!(p.contains("_path"));
        assert!(p.contains("Output ONLY the TOML"));
    }

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
    fn claude_not_found_error_mentions_claude() {
        let err = InitError::ClaudeNotFound;
        let msg = format!("{err}").to_lowercase();
        assert!(msg.contains("claude"));
    }

    /// `run` bails before spawning the agent when the output already exists and
    /// `--force` was not passed (the temp dir itself is an existing path).
    #[test]
    fn run_bails_when_output_exists_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let opts = InitOptions {
            root: dir.path().to_path_buf(),
            output: dir.path().to_path_buf(),
            force: false,
        };
        let err = run(opts).unwrap_err();
        assert!(
            matches!(err, InitError::AlreadyExists { .. }),
            "got: {err:?}"
        );
    }
}
