//! `dirsql init` subcommand: generate a starter `.dirsql.toml`, either
//! heuristically (template mode) or from an LLM response.
//!
//! The actual LLM HTTP call is intentionally NOT implemented here. Two
//! offline modes cover the same code path the in-process client would
//! eventually feed:
//!
//! - `--print-prompt` — emit the prompt the LLM should answer, to stdout.
//! - `--apply <file>` — read an LLM JSON response from `<file>` (or `-`
//!   for stdin) and write `.dirsql.toml` from it.
//!
//! This split keeps the entire feature testable without LLM credentials
//! and lets users pipe through any LLM CLI of their choice today. A
//! future end-to-end mode (`dirsql init --infer` with no sub-flag) will
//! perform the HTTP call internally.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::inference::{
    self, InferenceError, SampleOptions, build_prompt, parse_response, render_toml,
    sample_directory, template_from_summary, write_config,
};

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Directory to scan. Defaults to the current working directory.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// Where to write the generated `.dirsql.toml`. Defaults to
    /// `<root>/.dirsql.toml`.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Overwrite the output file if it already exists.
    #[arg(long)]
    pub force: bool,

    /// Use LLM-assisted inference instead of the heuristic template. Pair
    /// with `--print-prompt` (to emit the prompt for the LLM to answer)
    /// or `--apply <file>` (to consume the LLM's JSON response). The
    /// in-process HTTP client is not yet wired up; see
    /// `docs/guide/init.md` for the recommended pipeline.
    #[arg(long)]
    pub infer: bool,

    /// With `--infer`: print the LLM prompt to stdout and exit. Does not
    /// write any files.
    #[arg(long)]
    pub print_prompt: bool,

    /// With `--infer`: read a JSON LLM response from this path (or `-` to
    /// read from stdin), validate it, and write the resulting
    /// `.dirsql.toml`.
    #[arg(long)]
    pub apply: Option<PathBuf>,
}

/// Run the `init` subcommand. Returns a process exit code.
pub fn run(args: InitArgs) -> i32 {
    match dispatch(args) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("dirsql init: {err}");
            1
        }
    }
}

fn dispatch(args: InitArgs) -> Result<(), InitError> {
    let root = match &args.root {
        Some(p) => p.clone(),
        None => std::env::current_dir().map_err(InitError::Io)?,
    };
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| root.join(".dirsql.toml"));

    if args.print_prompt && args.apply.is_some() {
        return Err(InitError::ConflictingFlags(
            "--print-prompt and --apply are mutually exclusive",
        ));
    }
    if (args.print_prompt || args.apply.is_some()) && !args.infer {
        return Err(InitError::ConflictingFlags(
            "--print-prompt / --apply require --infer",
        ));
    }

    if args.infer {
        return run_infer(&root, &output, &args);
    }

    // Default: template mode.
    let summary = sample_directory(&root, &SampleOptions::default()).map_err(InitError::from)?;
    let cfg = template_from_summary(&summary);
    let toml = render_toml(&cfg);
    write_config(&output, &toml, args.force)?;
    println!("Wrote {} ({} table(s))", output.display(), cfg.tables.len());
    Ok(())
}

fn run_infer(root: &Path, output: &Path, args: &InitArgs) -> Result<(), InitError> {
    if args.print_prompt {
        let summary = sample_directory(root, &SampleOptions::default()).map_err(InitError::from)?;
        let prompt = build_prompt(&summary);
        // Use `print!` (not `println!`) so the prompt's own trailing
        // newline isn't doubled.
        print!("{prompt}");
        return Ok(());
    }

    if let Some(apply_path) = &args.apply {
        let json = read_apply_input(apply_path)?;
        let cfg = parse_response(&json).map_err(InitError::from)?;
        let toml = render_toml(&cfg);
        write_config(output, &toml, args.force)?;
        println!(
            "Wrote {} ({} table(s) from LLM response)",
            output.display(),
            cfg.tables.len()
        );
        return Ok(());
    }

    Err(InitError::ConflictingFlags(
        "`init --infer` needs --print-prompt or --apply <file>; the in-process HTTP client is not wired up yet (see docs/guide/init.md)",
    ))
}

fn read_apply_input(path: &Path) -> Result<String, InitError> {
    if path == Path::new("-") {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(InitError::Io)?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).map_err(InitError::Io)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error(transparent)]
    Io(io::Error),

    #[error(transparent)]
    Inference(#[from] inference::InferenceError),

    #[error("{0}")]
    ConflictingFlags(&'static str),
}

impl From<io::Error> for InitError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// Provide a clearer top-level error message for the most common
// `OutputExists` failure: the user just needs `--force`.
impl InitError {
    pub fn user_facing(&self) -> String {
        match self {
            Self::Inference(InferenceError::OutputExists(p)) => {
                format!("{} already exists; pass --force to overwrite", p.display())
            }
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn template_mode_writes_default_path() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.json"), "{}").unwrap();

        let args = InitArgs {
            root: Some(tmp.path().to_path_buf()),
            output: None,
            force: false,
            infer: false,
            print_prompt: false,
            apply: None,
        };
        assert_eq!(run(args), 0);
        assert!(tmp.path().join(".dirsql.toml").exists());
    }

    #[test]
    fn print_prompt_without_infer_errors() {
        let args = InitArgs {
            root: None,
            output: None,
            force: false,
            infer: false,
            print_prompt: true,
            apply: None,
        };
        assert_ne!(run(args), 0);
    }

    #[test]
    fn apply_with_invalid_json_errors() {
        let tmp = TempDir::new().unwrap();
        let resp = tmp.path().join("resp.json");
        fs::write(&resp, "not json").unwrap();
        let args = InitArgs {
            root: Some(tmp.path().to_path_buf()),
            output: None,
            force: false,
            infer: true,
            print_prompt: false,
            apply: Some(resp),
        };
        assert_ne!(run(args), 0);
        assert!(!tmp.path().join(".dirsql.toml").exists());
    }
}
