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
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::command::{self, Placeholder};
use crate::infer::{JsonRow, parse_rows_at};

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

/// [`run`], handing rows to `sink` as they parse. Invocations run
/// concurrently, like `xargs -P`, each decoding its own output as it arrives;
/// rows keep their order within an invocation but not across invocations.
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
    let mut runs = chunks(&args, ARG_BUDGET.saturating_sub(argv_bytes(&argv)));
    if runs.len() == 1 {
        runs = spread_runs(&args, &file_sizes(paths), cpus);
    }
    let workers = if runs.len() <= 1 {
        runs.len()
    } else {
        cpus().min(runs.len())
    };

    let next = AtomicUsize::new(0);
    let aborted = AtomicBool::new(false);
    let failures = Mutex::new(Vec::<(usize, String)>::new());
    std::thread::scope(|scope| {
        let (rows_tx, rows_rx) = std::sync::mpsc::sync_channel::<Vec<JsonRow>>(16);
        for _ in 0..workers {
            let rows_tx = rows_tx.clone();
            let (argv, runs) = (&argv, &runs);
            let (next, aborted, failures) = (&next, &aborted, &failures);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    if index >= runs.len() || aborted.load(Ordering::SeqCst) {
                        return;
                    }
                    let mut full = argv.clone();
                    full.extend(runs[index].iter().cloned());
                    let outcome = run_invocation(command, &full, cwd, &|rows| {
                        let _ = rows_tx.send(rows);
                    });
                    if let Err(message) = outcome {
                        aborted.store(true, Ordering::SeqCst);
                        failures
                            .lock()
                            .expect("failures lock")
                            .push((index, message));
                    }
                }
            });
        }
        drop(rows_tx);
        for rows in rows_rx {
            sink(rows);
        }
    });
    // The earliest invocation's failure outranks a later one's.
    match failures
        .into_inner()
        .expect("failures lock")
        .into_iter()
        .min()
    {
        Some((_, message)) => Err(message),
        None => Ok(()),
    }
}

/// One invocation: stream its stdout through the parser, `send`ing each
/// block's rows. A non-zero exit outranks bad output, and bad output is
/// drained rather than cut short so the child never dies on a broken pipe.
fn run_invocation(
    command: &str,
    full: &[String],
    cwd: &Path,
    send: &dyn Fn(Vec<JsonRow>),
) -> Result<(), String> {
    let mut bad_output: Option<String> = None;
    let mut lines_seen = 0usize;
    command::run_argv_blocks(command, full, cwd, &mut |block| {
        if bad_output.is_some() {
            return;
        }
        match parse_rows_at(block, lines_seen) {
            Ok(rows) => {
                lines_seen += block.bytes().filter(|&b| b == b'\n').count();
                if !rows.is_empty() {
                    send(rows);
                }
            }
            Err(message) => {
                bad_output = Some(format!(
                    "on-file output was not one JSON object per line: {message}"
                ));
            }
        }
    })
    .map_err(spawn_failure)?;
    bad_output.map_or(Ok(()), Err)
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

/// Bytes of file data worth a process of its own: below this, spawning
/// another invocation costs more than it saves.
const MIN_RUN_BYTES: u64 = 4 * 1024 * 1024;

fn cpus() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

/// [`spread`] over the machine's CPUs, asking for the count only when the
/// data could fill more than one run: the query reads cgroup files each time.
fn spread_runs<'a>(
    args: &'a [String],
    sizes: &[u64],
    cpus: impl FnOnce() -> usize,
) -> Vec<&'a [String]> {
    spread(args, sizes, cpus())
}

/// Each path's size in bytes; a path that cannot be read counts as empty.
fn file_sizes(paths: &[PathBuf]) -> Vec<u64> {
    paths
        .iter()
        .map(|path| std::fs::metadata(path).map_or(0, |meta| meta.len()))
        .collect()
}

/// Split `args` into at most `parts` consecutive runs holding about equal
/// bytes of file data, preserving order, and never more runs than
/// [`MIN_RUN_BYTES`] of data justify. `sizes` runs parallel to `args`.
fn spread<'a>(args: &'a [String], sizes: &[u64], parts: usize) -> Vec<&'a [String]> {
    let total: u64 = sizes.iter().sum();
    let parts = parts
        .min(args.len())
        .min(usize::try_from(total / MIN_RUN_BYTES).unwrap_or(usize::MAX));
    if parts <= 1 {
        return vec![args];
    }
    let mut out = Vec::new();
    let mut start = 0;
    let mut seen = 0u64;
    for (i, size) in sizes.iter().enumerate() {
        seen += size;
        let cut = u64::try_from(out.len() + 1).unwrap_or(u64::MAX);
        let last = i + 1 == args.len();
        if !last && out.len() + 1 < parts && seen * parts as u64 >= total * cut {
            out.push(&args[start..=i]);
            start = i + 1;
        }
    }
    out.push(&args[start..]);
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

    fn long_paths(count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                PathBuf::from(format!(
                    "/some/long/directory/name/for/the/budget/f-{i:06}.json"
                ))
            })
            .collect()
    }

    #[test]
    fn run_gives_every_path_to_some_invocation_when_the_table_needs_several() {
        let paths = long_paths(6000);
        let rows = run(
            r#"sh -c 'echo "{\"n\":$#}"' sh"#,
            Path::new("."),
            Path::new("."),
            &paths,
        )
        .unwrap();
        assert!(
            rows.len() > 1,
            "{} paths outgrow one argument list",
            paths.len()
        );
        let total: i64 = rows
            .iter()
            .map(|r| r.get("n").unwrap().as_i64().unwrap())
            .sum();
        assert_eq!(total, 6000);
    }

    fn files_of(dir: &Path, count: usize, bytes: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                let path = dir.join(format!("f{i}.jsonl"));
                std::fs::write(&path, vec![b'x'; bytes]).unwrap();
                path
            })
            .collect()
    }

    fn invocations_over(paths: &[PathBuf]) -> usize {
        run(
            r#"sh -c 'echo "{\"n\":$#}"' sh"#,
            Path::new("."),
            Path::new("."),
            paths,
        )
        .unwrap()
        .len()
    }

    #[test]
    fn spread_cuts_equal_bytes_in_order() {
        let args = args(&["a", "b", "c", "d"]);
        let mib = 1024 * 1024;
        let runs = spread(&args, &[8 * mib, 8 * mib, 8 * mib, 8 * mib], 2);
        assert_eq!(lens(&runs), [2, 2]);
        assert_eq!(runs[1], ["c", "d"]);
    }

    #[test]
    fn spread_never_makes_more_runs_than_the_data_justifies() {
        let args = args(&["a", "b", "c", "d"]);
        let mib = 1024 * 1024;
        assert_eq!(lens(&spread(&args, &[5 * mib; 4], 16)), [1, 1, 1, 1]);
        assert_eq!(lens(&spread(&args, &[mib; 4], 16)), [4]);
    }

    #[test]
    fn spread_runs_does_not_count_cpus_for_data_that_fits_one_run() {
        let args = args(&["a", "b", "c", "d"]);
        let mib = 1024 * 1024;
        let runs = spread_runs(&args, &[mib; 4], || panic!("counted the cpus"));
        assert_eq!(lens(&runs), [4]);
    }

    #[test]
    fn spread_runs_counts_cpus_once_the_data_fills_more_than_one_run() {
        let args = args(&["a", "b", "c", "d"]);
        let mib = 1024 * 1024;
        let runs = spread_runs(&args, &[8 * mib; 4], || 2);
        assert_eq!(lens(&runs), [2, 2]);
    }

    #[test]
    fn spread_isolates_a_dominant_file() {
        let args = args(&["a", "b", "c"]);
        let mib = 1024 * 1024;
        assert_eq!(lens(&spread(&args, &[100 * mib, mib, mib], 2)), [1, 2]);
    }

    #[test]
    fn spread_cuts_at_each_even_share_of_the_bytes() {
        let args = args(&["a", "b", "c", "d", "e"]);
        let mib = 1024 * 1024;
        let sizes = [5 * mib, 0, 5 * mib, 5 * mib, 0];
        assert_eq!(lens(&spread(&args, &sizes, 3)), [1, 2, 2]);
    }

    #[test]
    fn spread_never_ends_with_an_empty_run() {
        let args = args(&["a", "b", "c"]);
        let mib = 1024 * 1024;
        let sizes = [20 * mib, 0, 20 * mib];
        assert_eq!(lens(&spread(&args, &sizes, 3)), [1, 2]);
    }

    #[test]
    fn spread_gives_unreadable_files_no_weight() {
        let args = args(&["a", "b"]);
        assert_eq!(lens(&spread(&args, &[0, 0], 4)), [2]);
    }

    #[test]
    fn run_spreads_a_few_large_files_across_the_available_workers() {
        let dir = tempfile::tempdir().unwrap();
        let paths = files_of(dir.path(), 4, 4 * 1024 * 1024);
        let runs = invocations_over(&paths);
        assert!((2..=4).contains(&runs), "got {runs} invocations");
    }

    #[test]
    fn run_keeps_a_few_small_files_in_one_invocation() {
        let dir = tempfile::tempdir().unwrap();
        let paths = files_of(dir.path(), 4, 100);
        assert_eq!(invocations_over(&paths), 1);
    }

    #[test]
    fn run_names_the_line_of_bad_output_after_earlier_blocks() {
        let err = run(
            r#"sh -c 'yes "{\"a\":1}" | head -n 100000; echo nope' sh"#,
            Path::new("."),
            Path::new("."),
            &[PathBuf::from("/x")],
        )
        .unwrap_err();
        assert!(err.contains("line 100001 "), "got: {err}");
        assert!(err.contains("one JSON object per line"), "got: {err}");
    }

    #[test]
    fn run_prefers_a_non_zero_exit_to_the_bad_output_before_it() {
        let err = run(
            "sh -c 'echo nope; echo boom >&2; exit 4' sh",
            Path::new("."),
            Path::new("."),
            &[PathBuf::from("/x")],
        )
        .unwrap_err();
        assert!(err.contains("boom"), "got: {err}");
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
