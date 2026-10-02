//! The `on-file` runner: one process per table, every matched path appended
//! to the command's argv as trailing arguments, one JSON array of row objects
//! back on stdout.
//!
//! A table over more paths than one argv can carry is split into the fewest
//! invocations that fit, and their arrays are concatenated; the command never
//! sees the split. Zero paths spawn nothing. Any failure -- a spawn error, a
//! non-zero exit, empty output, or output that is not an array of objects --
//! fails the whole table.

use std::path::{Path, PathBuf};

use crate::command::{self, Placeholder};
use crate::infer::{JsonRow, parse_rows};

/// Bytes of trailing arguments one invocation may carry: GNU xargs's ceiling,
/// comfortably under every platform's argv limit with the environment and the
/// template's own argv on top.
#[cfg(windows)]
const ARG_BUDGET: usize = 32_000;
#[cfg(not(windows))]
const ARG_BUDGET: usize = 128 * 1024;

const PATH_PLACEHOLDER: &str = "{path}";

/// The load-time rejection for a command still written to the retired
/// one-process-per-file contract, or `None` when the command is fine.
pub(crate) fn path_placeholder_rejection(command: &str) -> Option<String> {
    command.contains(PATH_PLACEHOLDER).then(|| {
        format!(
            "on-file command `{command}` uses `{PATH_PLACEHOLDER}`, but an on-file command \
             now runs once per table with every matched path appended as trailing \
             arguments and prints one JSON array of row objects. Remove \
             `{PATH_PLACEHOLDER}` and read the paths from the command's arguments."
        )
    })
}

/// Run `command` over `paths` and parse everything it printed into rows.
///
/// `cwd` is the child's working directory and `root` the index root offered
/// as the `{root}` placeholder. The error is a one-line message naming the
/// stage that failed, carrying the tail of the command's stderr.
pub(crate) fn run(
    command: &str,
    cwd: &Path,
    root: &Path,
    paths: &[PathBuf],
) -> Result<Vec<JsonRow>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = [Placeholder::path("root", &root.to_string_lossy())];
    let argv = command::build_argv(command, &placeholders).map_err(spawn_failure)?;
    let args: Vec<String> = paths
        .iter()
        .map(|path| command::non_verbatim(&path.to_string_lossy()))
        .collect();

    let mut rows = Vec::new();
    for chunk in chunks(&args, ARG_BUDGET.saturating_sub(argv_bytes(&argv))) {
        let mut full = argv.clone();
        full.extend(chunk.iter().cloned());
        let output = command::run_argv(command, &full, cwd, None).map_err(spawn_failure)?;
        let parsed = parse_rows(&output.payload)
            .map_err(|message| format!("on-file output was not a JSON array of rows: {message}"))?;
        rows.extend(parsed);
    }
    Ok(rows)
}

fn spawn_failure(error: command::CommandError) -> String {
    format!("on-file command failed: {error}")
}

/// The bytes `argv` costs: each argument plus its terminator.
fn argv_bytes(argv: &[String]) -> usize {
    argv.iter().map(|arg| arg.len() + 1).sum()
}

/// Split `args` into the fewest consecutive runs whose bytes fit `budget`,
/// preserving order. A single argument over the budget still gets a run of
/// its own: the kernel, not this split, is the judge of what fits.
fn chunks(args: &[String], budget: usize) -> Vec<&[String]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (i, arg) in args.iter().enumerate() {
        let cost = arg.len() + 1;
        if i > start && used + cost > budget {
            out.push(&args[start..i]);
            start = i;
            used = 0;
        }
        used += cost;
    }
    if start < args.len() {
        out.push(&args[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn lens(chunks: &[&[String]]) -> Vec<usize> {
        chunks.iter().map(|chunk| chunk.len()).collect()
    }

    #[test]
    fn chunks_keeps_everything_in_one_run_when_it_fits() {
        let args = args(&["aa", "bb", "cc"]);
        assert_eq!(lens(&chunks(&args, 9)), vec![3]);
    }

    #[test]
    fn chunks_splits_at_the_budget_preserving_order() {
        let args = args(&["aa", "bb", "cc", "dd"]);
        let split = chunks(&args, 6);
        assert_eq!(lens(&split), vec![2, 2]);
        assert_eq!(split[0], &args[..2]);
        assert_eq!(split[1], &args[2..]);
    }

    #[test]
    fn chunks_counts_the_terminator_of_every_argument() {
        let args = args(&["aa", "bb"]);
        assert_eq!(lens(&chunks(&args, 5)), vec![1, 1], "2+1 twice exceeds 5");
        assert_eq!(lens(&chunks(&args, 6)), vec![2], "6 fits 2+1 twice");
    }

    #[test]
    fn chunks_gives_an_oversized_argument_a_run_of_its_own() {
        let args = args(&["a", "toolong", "b"]);
        assert_eq!(lens(&chunks(&args, 3)), vec![1, 1, 1]);
    }

    #[test]
    fn chunks_never_emits_an_empty_run_before_a_leading_oversized_argument() {
        let args = args(&["toolong", "b"]);
        assert_eq!(lens(&chunks(&args, 3)), vec![1, 1]);
    }

    #[test]
    fn the_budget_carries_hundreds_of_paths_in_one_run() {
        let paths: Vec<String> = (0..300).map(|i| format!("/{i:0>99}")).collect();
        assert_eq!(lens(&chunks(&paths, ARG_BUDGET)), vec![300]);
    }

    #[test]
    fn chunks_over_nothing_is_no_runs() {
        assert!(chunks(&[], 10).is_empty());
    }

    #[test]
    fn argv_bytes_sums_each_argument_and_its_terminator() {
        assert_eq!(argv_bytes(&args(&["sh", "run.sh"])), 3 + 7);
    }

    #[test]
    fn path_placeholder_is_rejected_naming_the_new_contract() {
        let message = path_placeholder_rejection("cat {path}").expect("rejected");
        assert!(
            message.contains("cat {path}"),
            "names the command: {message}"
        );
        assert!(
            message.contains("trailing arguments"),
            "names the contract: {message}"
        );
        assert!(
            message.contains("one JSON array"),
            "names the output: {message}"
        );
    }

    #[test]
    fn a_command_without_the_placeholder_is_accepted() {
        assert_eq!(path_placeholder_rejection("python3 extract.py"), None);
        assert_eq!(path_placeholder_rejection("sh run.sh {root}"), None);
    }

    #[test]
    fn run_over_no_paths_spawns_nothing() {
        let rows = run(
            "definitely-not-a-real-binary-xyzzy",
            Path::new("."),
            Path::new("."),
            &[],
        );
        assert_eq!(rows.unwrap(), Vec::new());
    }
}
