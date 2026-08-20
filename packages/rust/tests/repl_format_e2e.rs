//! End-to-end tests for how result rows are rendered (#989).
//!
//! These spawn the real compiled `dirsql` binary over a fixture directory and
//! assert what reaches stdout. Nothing is mocked (real process, real
//! filesystem, real SQLite).
//!
//! The contract under test: a JSON array is right for `dirsql query "…" | jq`
//! and wrong for a human reading a REPL, so the rendering follows the
//! **destination**. `--format` decides, defaulting to `auto`, which reads
//! stdout: a table when it is a terminal, JSON when it is redirected. The
//! flag is valid in one-shot mode and the REPL alike; `auto` in a pipe must
//! stay byte-identical to what `dirsql query` printed before this existed,
//! which is the regression that matters most.
//!
//! Every case here forces the format explicitly or runs through a pipe, so
//! none of it needs a terminal.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Output, Stdio};

use assert_cmd::prelude::*;
use tempfile::TempDir;

/// Two files with known, distinct sizes, so a rendering can be pinned exactly.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("a.md"), "alpha\n").unwrap();
    std::fs::write(root.path().join("bb.md"), "beta beta\n").unwrap();
    root
}

/// Run a one-shot query with the given extra argv.
fn query(root: &TempDir, sql: &str, args: &[&str]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg(sql)
        .args(args)
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed")
}

/// Run the REPL over a pipe with the given extra argv.
fn repl(root: &TempDir, stdin: &str, args: &[&str]) -> Output {
    let mut child = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning `dirsql` failed");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(stdin.as_bytes())
        .expect("writing to the REPL's stdin failed");
    child
        .wait_with_output()
        .expect("waiting on `dirsql` failed")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const SQL: &str = "SELECT basename, size FROM './' ORDER BY basename";

#[test]
fn a_piped_one_shot_query_still_prints_the_json_array() {
    // The regression that matters most: `dirsql "…" | jq` must not change.
    let root = fixture();

    let out = query(&root, SQL, &[]);

    assert_eq!(
        stdout_of(&out).trim(),
        r#"[{"basename":"a.md","size":6},{"basename":"bb.md","size":10}]"#,
        "{out:?}"
    );
}

#[test]
fn auto_in_a_pipe_is_byte_identical_to_explicit_json() {
    // `auto` reads the destination; a pipe is not a terminal, so the two must
    // produce the same bytes rather than merely similar ones.
    let root = fixture();

    let inferred = query(&root, SQL, &[]);
    let explicit = query(&root, SQL, &["--format", "json"]);

    assert_eq!(
        inferred.stdout, explicit.stdout,
        "{inferred:?} {explicit:?}"
    );
}

#[test]
fn the_table_format_can_be_asked_for_in_a_pipe() {
    // The point of a flag rather than TTY-only inference: the user decides,
    // in both directions.
    let root = fixture();

    let out = query(&root, SQL, &["--format", "table"]);
    let stdout = stdout_of(&out);

    assert!(stdout.contains("basename"), "a header row, got {stdout:?}");
    assert!(stdout.contains("a.md"), "got {stdout:?}");
    assert!(stdout.contains("bb.md"), "got {stdout:?}");
    assert!(
        !stdout.contains(r#"{"basename""#),
        "no JSON in table mode, got {stdout:?}"
    );
}

#[test]
fn the_table_aligns_its_columns() {
    // A table whose columns do not line up is just noisier JSON. Both value
    // rows must place their second column at the same offset.
    let root = fixture();

    let out = query(&root, SQL, &["--format", "table"]);
    let stdout = stdout_of(&out);
    let offsets: Vec<Option<usize>> = stdout
        .lines()
        .filter(|line| line.contains("a.md") || line.contains("bb.md"))
        .map(|line| line.rfind("6").or_else(|| line.rfind("10")))
        .collect();

    assert_eq!(offsets.len(), 2, "two value rows, got {stdout:?}");
    assert_eq!(offsets[0], offsets[1], "columns must align, got {stdout:?}");
}

#[test]
fn the_table_reports_how_many_rows_there_were() {
    // Reading rows with eyes means wanting the count without counting.
    let root = fixture();

    let out = query(&root, SQL, &["--format", "table"]);

    assert!(
        stdout_of(&out).contains("2 rows"),
        "got {}",
        stdout_of(&out)
    );
}

#[test]
fn an_empty_table_says_so_rather_than_printing_nothing() {
    let root = fixture();

    let out = query(
        &root,
        "SELECT basename FROM './' WHERE 0",
        &["--format", "table"],
    );

    assert!(
        stdout_of(&out).contains("no rows"),
        "got {}",
        stdout_of(&out)
    );
}

#[test]
fn a_null_is_named_rather_than_rendered_as_a_blank() {
    // A blank cell is indistinguishable from an empty string.
    let root = fixture();

    let out = query(&root, "SELECT NULL AS nothing", &["--format", "table"]);

    assert!(stdout_of(&out).contains("NULL"), "got {}", stdout_of(&out));
}

#[test]
fn the_json_format_can_be_asked_for_explicitly() {
    let root = fixture();

    let out = query(&root, SQL, &["--format", "json"]);

    assert_eq!(
        stdout_of(&out).trim(),
        r#"[{"basename":"a.md","size":6},{"basename":"bb.md","size":10}]"#,
        "{out:?}"
    );
}

#[test]
fn the_repl_takes_the_same_flag() {
    // `--format` is valid wherever rows are printed, not just in one-shot
    // mode -- the REPL is the surface that motivated it.
    let root = fixture();

    let out = repl(
        &root,
        "SELECT basename FROM './' ORDER BY basename\n",
        &["--format", "table"],
    );
    let stdout = stdout_of(&out);

    assert!(stdout.contains("basename"), "a header row, got {stdout:?}");
    assert!(
        !stdout.contains(r#"{"basename""#),
        "no JSON in table mode, got {stdout:?}"
    );
}

#[test]
fn the_piped_repl_defaults_to_json() {
    // `auto` keys on stdout, and the REPL's stdout here is a pipe.
    let root = fixture();

    let out = repl(&root, "SELECT basename FROM './' ORDER BY basename\n", &[]);

    assert_eq!(
        stdout_of(&out).trim(),
        r#"[{"basename":"a.md"},{"basename":"bb.md"}]"#,
        "{out:?}"
    );
}

#[test]
fn an_unknown_format_is_a_usage_error() {
    // Silently falling back would hide a typo until someone read the output.
    let root = fixture();

    let out = query(&root, SQL, &["--format", "yaml"]);

    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("yaml"),
        "the error names the value, got {out:?}"
    );
}

#[test]
fn the_server_does_not_take_the_flag() {
    // The server speaks JSON over HTTP; a rendering flag there would be a
    // promise the transport cannot keep.
    let root = fixture();

    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(["server", "--format", "table"])
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");

    assert_eq!(out.status.code(), Some(2), "{out:?}");
}
