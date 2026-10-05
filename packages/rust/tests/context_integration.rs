//! Integration tests for `dirsql context`, the agent usage guide compiled
//! into the binary. Spawns the real `dirsql` binary.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;

fn run_context() -> std::process::Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("context")
        .output()
        .expect("spawning dirsql failed")
}

#[test]
fn context_exits_zero_with_nothing_on_stderr() {
    let out = run_context();
    assert!(
        out.status.success(),
        "dirsql context failed: status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        out.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn context_starts_with_the_running_version() {
    let out = run_context();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().next(),
        Some(format!("dirsql {}", env!("CARGO_PKG_VERSION")).as_str()),
        "first line must name the running version; stdout:\n{stdout}"
    );
}

#[test]
fn context_prints_usage_recipes_and_troubleshooting_in_order() {
    let out = run_context();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let position = |heading: &str| {
        stdout
            .lines()
            .position(|line| line == heading)
            .unwrap_or_else(|| panic!("missing `{heading}` heading; stdout:\n{stdout}"))
    };
    let usage = position("## Usage");
    let recipes = position("## Recipes");
    let troubleshooting = position("## Troubleshooting");
    assert!(
        usage < recipes && recipes < troubleshooting,
        "sections out of order: usage={usage} recipes={recipes} troubleshooting={troubleshooting}"
    );
}

#[test]
fn context_takes_no_arguments() {
    let out = std::process::Command::cargo_bin("dirsql")
        .unwrap()
        .args(["context", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "a flag after `context` must be a usage error; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}
