//! End-to-end tests for file names that are not valid Unicode: the real
//! `dirsql` binary must keep each one distinct in the path columns and hand
//! the `on-file` command the name as it is on disk. Nothing is mocked (real
//! process, real filesystem, real SQLite, a real Python hook).
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;

/// A file name holding `unit` where a character should be: a byte that is
/// not UTF-8 on Unix, an unpaired surrogate on Windows.
fn not_unicode(unit: u16) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let byte = u8::try_from(unit & 0xff).unwrap();
        OsString::from_vec([b"ok".as_slice(), &[byte], b"name.txt"].concat())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let wide: Vec<u16> = "ok"
            .encode_utf16()
            .chain([unit])
            .chain("name.txt".encode_utf16())
            .collect();
        OsString::from_wide(&wide)
    }
}

/// The names as SQLite's `hex()` spells their bytes.
fn hex(name: &OsString) -> String {
    name.as_encoded_bytes()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// Two files whose names differ only in their non-Unicode unit, sized 1 and
/// 2 bytes, returned with their names in size order.
fn fixture() -> (TempDir, [OsString; 2]) {
    let root = TempDir::new().unwrap();
    let names = [not_unicode(0xD800 | 0xff), not_unicode(0xD800 | 0xfe)];
    std::fs::write(root.path().join(&names[0]), "A").unwrap();
    std::fs::write(root.path().join(&names[1]), "BB").unwrap();
    (root, names)
}

fn dirsql(root: &Path, args: &[&str]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .args(["--format", "json"])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root)
        .output()
        .expect("spawning `dirsql` failed")
}

fn rows(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout must be JSON")
}

#[test]
fn each_name_keeps_its_own_bytes_in_the_path_columns() {
    let (root, names) = fixture();
    let out = dirsql(
        root.path(),
        &["SELECT hex(path) AS p, hex(basename) AS b, size FROM './*' ORDER BY size"],
    );
    assert_eq!(
        rows(&out),
        json!([
            {"p": hex(&names[0]), "b": hex(&names[0]), "size": 1},
            {"p": hex(&names[1]), "b": hex(&names[1]), "size": 2},
        ])
    );
}

#[test]
fn reading_such_a_path_as_text_fails_naming_the_column() {
    let (root, _) = fixture();
    let out = dirsql(root.path(), &["SELECT path FROM './*'"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        stderr.contains("column \"path\" holds text that is not valid UTF-8"),
        "stderr: {stderr}"
    );
}

#[test]
fn an_equality_on_the_path_finds_the_file() {
    let (root, names) = fixture();
    let sql = format!(
        "SELECT size FROM './*' WHERE path = CAST(x'{}' AS TEXT)",
        hex(&names[1])
    );
    assert_eq!(rows(&dirsql(root.path(), &[&sql])), json!([{"size": 2}]));
}

#[test]
fn the_on_file_command_receives_each_name_as_it_is_on_disk() {
    let (root, names) = fixture();
    let hooks = TempDir::new().unwrap();
    let hook = hooks.path().join("hook.py");
    std::fs::write(
        &hook,
        "import json, os, sys\n\
         for p in sys.argv[1:]:\n    \
         print(json.dumps({\"found\": os.path.isfile(p), \
         \"name\": os.fsencode(os.path.basename(p)).hex().upper()}))\n",
    )
    .unwrap();
    let command = format!("{} '{}'", python(), hook.display());
    let out = dirsql(
        root.path(),
        &[
            "query",
            "SELECT found, name FROM './*' ORDER BY name DESC",
            "--on-file",
            &command,
        ],
    );
    assert_eq!(
        rows(&out),
        json!([
            {"found": 1, "name": hex(&names[0])},
            {"found": 1, "name": hex(&names[1])},
        ])
    );
}
