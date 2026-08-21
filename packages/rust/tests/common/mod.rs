//! Helpers shared across integration and e2e test binaries.

use std::path::PathBuf;
use std::process::Command;

/// Build the test-only loadable extension fixture
/// (`tests/fixtures/testext`) and return the path to its compiled shared
/// library. Built into `CARGO_TARGET_TMPDIR` with coverage/instrumentation
/// env removed so the nested build is independent of an outer
/// `cargo llvm-cov` invocation.
pub fn build_fixture_extension() -> PathBuf {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/testext/Cargo.toml"
    );
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--manifest-path",
            manifest,
            "--target-dir",
            env!("CARGO_TARGET_TMPDIR"),
            "--message-format=json",
        ])
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("failed to spawn cargo build for the extension fixture");
    assert!(
        output.status.success(),
        "fixture build failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    // Parse cargo's JSON output for the cdylib artifact path (platform-
    // independent: .so / .dylib / .dll).
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut artifact: Option<PathBuf> = None;
    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg["reason"] == "compiler-artifact" {
            if let Some(files) = msg["filenames"].as_array() {
                for f in files.iter().filter_map(|f| f.as_str()) {
                    if f.ends_with(".so") || f.ends_with(".dylib") || f.ends_with(".dll") {
                        artifact = Some(PathBuf::from(f));
                    }
                }
            }
        }
    }
    artifact.expect("no cdylib artifact in cargo build output for the fixture")
}
