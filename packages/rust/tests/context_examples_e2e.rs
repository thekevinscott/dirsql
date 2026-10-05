//! Drift guard for `dirsql context`: every fenced `uvx dirsql` example the
//! guide prints runs against the real binary over a real fixture tree and
//! must exit 0 with a JSON array on stdout.

#![cfg(feature = "cli")]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

const RUNNABLE: &str = "uvx dirsql ";

// The embeddings plugin downloads a model on first use, so its example cannot
// run here; it is the only fenced command allowed to skip the guard.
const PLUGIN_ONLY: &str = "uvx --with dirsql-plugin-embeddings dirsql ";

fn guide() -> String {
    let out = std::process::Command::cargo_bin("dirsql")
        .unwrap()
        .arg("context")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "dirsql context failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut body: Option<String> = None;
    for line in text.lines() {
        let fence = line.trim_start().starts_with("```");
        match body.take() {
            Some(b) if fence => blocks.push(b),
            Some(mut b) => {
                b.push_str(line.trim_start());
                b.push('\n');
                body = Some(b);
            }
            None if fence => body = Some(String::new()),
            None => {}
        }
    }
    assert!(body.is_none(), "unterminated fence in the guide");
    blocks
}

/// Whitespace separates words; single or double quotes group them.
fn shell_words(cmd: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            None => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    assert!(quote.is_none(), "unbalanced quote in: {cmd}");
    if in_word {
        words.push(cur);
    }
    words
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    let r = root.path();
    write(r, "README.md", "# Fixture\n\nTODO: describe\n");
    write(
        r,
        "docs/intro.md",
        "---\ntitle: Introduction\nstatus: draft\n---\n\n# Intro\n\nTODO: write\nTODO: review\n",
    );
    write(
        r,
        "docs/guide/setup.md",
        "---\ntitle: Setup\nstatus: published\n---\n\n# Setup\n",
    );
    write(r, "docs/app.md", "# app\n");
    write(r, "src/app.py", "print('app')\n");
    write(r, "src/db.py", "print('db')\n");
    write(
        r,
        "plugins/alpha/metadata.json",
        r#"{"name":"alpha","version":"1.4.0","disabled":false,"tags":["search","index"]}"#,
    );
    write(
        r,
        "plugins/beta/metadata.json",
        r#"{"name":"beta","version":"0.9.2","disabled":true,"tags":["export"]}"#,
    );
    write(r, "plugins/empty/metadata.json", "");
    write(r, "data/big.bin", &"x".repeat(4096));
    write(r, ".gitignore", "build/\n");
    write(r, "build/out.bin", "built");
    write(r, "node_modules/dep/index.js", "module.exports = 1;\n");
    fs::File::options()
        .write(true)
        .open(r.join("docs/app.md"))
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(400 * 86_400))
        .unwrap();
    root
}

#[test]
fn every_fenced_example_in_the_guide_runs_clean() {
    let dir = fixture();
    let mut ran = 0;
    for block in fenced_blocks(&guide()) {
        let cmd = block.trim();
        if cmd.starts_with(PLUGIN_ONLY) {
            continue;
        }
        let args = cmd.strip_prefix(RUNNABLE).unwrap_or_else(|| {
            panic!("a fenced example must start with `{RUNNABLE}` or `{PLUGIN_ONLY}`: {cmd}")
        });
        let args = shell_words(args);
        assert!(
            args.windows(2).any(|w| w == ["--format", "json"]),
            "example must pass `--format json`: {cmd}"
        );
        let out = std::process::Command::cargo_bin("dirsql")
            .unwrap()
            .args(&args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "example failed ({}):\n{cmd}\nstderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        let rows: Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{cmd}"));
        assert!(rows.is_array(), "expected a JSON array:\n{cmd}");
        ran += 1;
    }
    assert!(
        ran >= 10,
        "expected at least 10 runnable examples, ran {ran}"
    );
}
