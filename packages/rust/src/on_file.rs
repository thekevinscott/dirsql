//! The `on-file` runner: one process per table, every matched path appended
//! to the command's argv as trailing arguments, one JSON object per line
//! (NDJSON) back on stdout.
//!
//! A table over more paths than one argv can carry is split into the fewest
//! invocations that fit, and their rows are concatenated; the command never
//! sees the split. Zero paths spawn nothing. Any failure -- a spawn error, a
//! non-zero exit, or output that is not one JSON object per line --
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
             arguments and prints one JSON object per line. Remove \
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
    let mut rows = Vec::new();
    run_streaming(command, cwd, root, paths, &mut |chunk| rows.extend(chunk))?;
    Ok(rows)
}

/// [`run`], handing each invocation's rows to `sink` as they parse. The
/// invocations still run one after another; parsing one invocation's output
/// overlaps the next invocation, the way a shell pipe would.
pub(crate) fn run_streaming(
    command: &str,
    cwd: &Path,
    root: &Path,
    paths: &[PathBuf],
    sink: &mut (dyn FnMut(Vec<JsonRow>) + Send),
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let placeholders = [Placeholder::path("root", &root.to_string_lossy())];
    let argv = command::build_argv(command, &placeholders).map_err(spawn_failure)?;
    let args: Vec<String> = paths
        .iter()
        .map(|path| command::non_verbatim(&path.to_string_lossy()))
        .collect();

    std::thread::scope(|scope| {
        let (payloads, received) = std::sync::mpsc::channel::<String>();
        let parser = scope.spawn(move || -> Result<(), String> {
            for payload in received {
                let rows = parse_rows(&payload).map_err(|message| {
                    format!("on-file output was not one JSON object per line: {message}")
                })?;
                sink(rows);
            }
            Ok(())
        });
        let spawned = (|| -> Result<(), String> {
            for chunk in chunks(&args, ARG_BUDGET.saturating_sub(argv_bytes(&argv))) {
                let mut full = argv.clone();
                full.extend(chunk.iter().cloned());
                let output =
                    command::run_argv_stdout(command, &full, cwd, None).map_err(spawn_failure)?;
                if payloads.send(output).is_err() {
                    break;
                }
            }
            Ok(())
        })();
        drop(payloads);
        // An earlier invocation's bad output outranks a later one's failure.
        parser.join().expect("parser thread panicked")?;
        spawned
    })
}

fn spawn_failure(error: command::CommandError) -> String {
    format!("on-file command failed: {error}")
}

/// The bytes `arg` costs on the command line: its spawned form plus a
/// separator. Windows wraps every argument in quotes, so the bytes handed to
/// the kernel exceed the bytes of the argument itself.
// One fn with cfg blocks rather than two cfg'd fns: cargo-mutants mutates
// the uncompiled twin too, and no test on a Linux gate can kill that mutant.
fn arg_cost(arg: &str) -> usize {
    #[cfg(windows)]
    let spawned = command::quote_windows_arg(arg).len();
    #[cfg(not(windows))]
    let spawned = arg.len();
    spawned + 1
}

/// The bytes `argv` costs: every argument at its [`arg_cost`].
fn argv_bytes(argv: &[String]) -> usize {
    argv.iter().map(|arg| arg_cost(arg)).sum()
}

/// Split `args` into the fewest consecutive runs whose bytes fit `budget`,
/// preserving order. A single argument over the budget still gets a run of
/// its own: the kernel, not this split, is the judge of what fits.
fn chunks(args: &[String], budget: usize) -> Vec<&[String]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (i, arg) in args.iter().enumerate() {
        let cost = arg_cost(arg);
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
    fn arg_cost_is_the_spawned_form_plus_a_separator() {
        let quotes = if cfg!(windows) { 2 } else { 0 };
        assert_eq!(arg_cost("aa"), 2 + quotes + 1);
    }

    #[test]
    fn chunks_keeps_everything_in_one_run_when_it_fits() {
        let args = args(&["aa", "bb", "cc"]);
        assert_eq!(lens(&chunks(&args, 3 * arg_cost("aa"))), vec![3]);
    }

    #[test]
    fn chunks_splits_at_the_budget_preserving_order() {
        let args = args(&["aa", "bb", "cc", "dd"]);
        let split = chunks(&args, 2 * arg_cost("aa"));
        assert_eq!(lens(&split), vec![2, 2]);
        assert_eq!(split[0], &args[..2]);
        assert_eq!(split[1], &args[2..]);
    }

    #[test]
    fn chunks_charges_every_argument_its_full_cost() {
        let args = args(&["aa", "bb"]);
        let two = 2 * arg_cost("aa");
        assert_eq!(lens(&chunks(&args, two - 1)), vec![1, 1], "one byte short");
        assert_eq!(lens(&chunks(&args, two)), vec![2], "exactly two fit");
    }

    #[test]
    fn chunks_gives_an_oversized_argument_a_run_of_its_own() {
        let args = args(&["a", "toolong", "b"]);
        assert_eq!(lens(&chunks(&args, arg_cost("a"))), vec![1, 1, 1]);
    }

    #[test]
    fn chunks_never_emits_an_empty_run_before_an_oversized_first_argument() {
        let args = args(&["toolong", "a"]);
        let split = chunks(&args, arg_cost("a"));
        assert_eq!(lens(&split), vec![1, 1]);
        assert_eq!(split[0], &args[..1]);
    }

    #[test]
    fn chunks_over_nothing_is_no_runs() {
        assert!(chunks(&[], 10).is_empty());
    }

    #[test]
    fn argv_bytes_sums_the_cost_of_every_argument() {
        assert_eq!(
            argv_bytes(&args(&["sh", "run.sh"])),
            arg_cost("sh") + arg_cost("run.sh")
        );
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
            message.contains("one JSON object per line"),
            "names the output: {message}"
        );
    }

    #[test]
    fn a_command_without_the_placeholder_is_accepted() {
        assert_eq!(path_placeholder_rejection("python3 extract.py"), None);
        assert_eq!(path_placeholder_rejection("sh run.sh {root}"), None);
    }

    /// The budget carries a real table's worth of paths in one spawn: a
    /// few hundred paths totalling well over ten kilobytes reach the command
    /// as one argument list.
    #[test]
    fn run_hands_a_large_table_to_one_spawn() {
        let paths: Vec<PathBuf> = (0..300)
            .map(|i| PathBuf::from(format!("/some/long/directory/name/file-{i:04}.json")))
            .collect();
        let rows = run(
            r#"sh -c 'echo "{\"n\":$#}"' sh"#,
            Path::new("."),
            Path::new("."),
            &paths,
        )
        .unwrap();
        assert_eq!(rows.len(), 1, "one spawn, one row");
        assert_eq!(rows[0].get("n").unwrap().as_i64().unwrap(), 300);
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
