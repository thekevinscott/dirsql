//! The bundled frontend's script, exercised by Node's built-in test runner.

use std::path::Path;
use std::process::Command;

#[test]
fn frontend_script_passes_its_node_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/frontend");
    let output = Command::new("node")
        .arg("--test")
        .arg(dir.join("query.test.mjs"))
        .arg(dir.join("browse.test.mjs"))
        .output()
        .expect("node must be installed to test the bundled frontend");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
