//! What the published `.crate` archive ships.
//!
//! `changelog.d/` and `migrations.d/` are repo *inputs* -- one fragment file
//! per change, assembled by nothing and read by no consumer of the crate.
//! The npm `files` allowlist and maturin's wheel layout keep them out of
//! their artifacts structurally; cargo ships everything not named in
//! `[package].exclude`, so the crate needs the patterns spelled out.
//!
//! Asserted against `cargo package --list` -- the archive's own manifest --
//! rather than against the `exclude` patterns, so a pattern that parses but
//! matches nothing still fails here.

use std::process::Command;

/// Every path `cargo package` would write into the `.crate` archive.
fn packaged_paths() -> Vec<String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            "--list",
        ])
        .output()
        .expect("cargo package --list should run");

    assert!(
        output.status.success(),
        "cargo package --list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .expect("cargo package --list emits utf-8")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Archive paths under `prefix`.
fn shipped_under(prefix: &str) -> Vec<String> {
    packaged_paths()
        .into_iter()
        .filter(|path| path.starts_with(prefix))
        .collect()
}

/// Guards the two assertions below against a vacuous pass: a `--list` that
/// silently returned nothing would otherwise satisfy every "does not ship"
/// check in this file.
#[test]
fn crate_archive_ships_the_library_source() {
    let paths = packaged_paths();

    assert!(
        paths.iter().any(|p| p == "src/lib.rs"),
        "expected src/lib.rs in the archive, got {} paths: {paths:?}",
        paths.len()
    );
}

#[test]
fn crate_archive_omits_changelog_fragments() {
    let shipped = shipped_under("changelog.d/");

    assert!(
        shipped.is_empty(),
        "changelog fragments are repo inputs and must not ship; \
         add `changelog.d/` to [package].exclude. Shipped: {shipped:?}"
    );
}

#[test]
fn crate_archive_omits_migration_fragments() {
    let shipped = shipped_under("migrations.d/");

    assert!(
        shipped.is_empty(),
        "migration fragments are repo inputs and must not ship; \
         add `migrations.d/` to [package].exclude. Shipped: {shipped:?}"
    );
}
