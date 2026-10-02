//! Drift guard for the `/dirsql` agent skill: every fenced `dirsql` command
//! in `.claude/skills/dirsql/` runs against the real binary over a real
//! fixture tree and must exit 0 with a JSON array on stdout. A skill that
//! drifts from the CLI is worse than none.

#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

const RUN_PATH: &str = "uvx dirsql ";

fn skill_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/skills/dirsql")
}

fn skill_files() -> Vec<PathBuf> {
    let dir = skill_dir();
    let skill = dir.join("SKILL.md");
    assert!(skill.is_file(), "skill file missing: {}", skill.display());
    let mut files = vec![skill];
    if let Ok(entries) = fs::read_dir(dir.join("references")) {
        let mut refs: Vec<PathBuf> = entries
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        refs.sort();
        files.extend(refs);
    }
    files
}

/// The bodies of every ```dirsql fenced block in `text`.
fn fenced_dirsql_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut body: Option<String> = None;
    for line in text.lines() {
        match &mut body {
            Some(b) if line.trim() == "```" => blocks.push(std::mem::take(b)),
            Some(b) => {
                b.push_str(line);
                b.push('\n');
            }
            None if line.trim() == "```dirsql" => body = Some(String::new()),
            None => {}
        }
        if line.trim() == "```" {
            body = None;
        }
    }
    blocks
}

/// Shell-style word splitting: whitespace separates, single or double
/// quotes group (newlines inside quotes are kept).
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
    write(r, "docs/app.md", "# app\n\nDocumented module.\n");
    write(r, "src/app.py", "print('app')\n");
    write(r, "src/db.py", "print('db')\n");
    write(r, "src/util.js", "console.log('util')\n");
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
    write(r, "data/big.bin", &"x".repeat(4096));
    write(r, ".gitignore", "build/\n");
    write(r, "build/out.bin", "built");
    let stale = SystemTime::now() - Duration::from_secs(400 * 86_400);
    fs::File::options()
        .write(true)
        .open(r.join("docs/app.md"))
        .unwrap()
        .set_modified(stale)
        .unwrap();
    root
}

#[test]
fn every_fenced_dirsql_example_in_the_skill_runs_clean() {
    let dir = fixture();
    let mut ran = 0;
    for file in skill_files() {
        let text = fs::read_to_string(&file).unwrap();
        for block in fenced_dirsql_blocks(&text) {
            let cmd = block.trim();
            let args = cmd.strip_prefix(RUN_PATH).unwrap_or_else(|| {
                panic!(
                    "{}: example must start with `{RUN_PATH}`: {cmd}",
                    file.display()
                )
            });
            let args = shell_words(args);
            assert!(
                args.windows(2).any(|w| w == ["--format", "json"]),
                "{}: example must pin `--format json`: {cmd}",
                file.display()
            );
            let out = std::process::Command::cargo_bin("dirsql")
                .unwrap()
                .args(&args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}: example failed ({}):\n{cmd}\nstderr: {}",
                file.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
            let rows: Value = serde_json::from_slice(&out.stdout)
                .unwrap_or_else(|e| panic!("{}: stdout is not JSON ({e}):\n{cmd}", file.display()));
            assert!(
                rows.is_array(),
                "{}: expected a JSON array:\n{cmd}",
                file.display()
            );
            ran += 1;
        }
    }
    assert!(ran > 0, "the skill ships no runnable examples");
}
