//! The example table in `docs/reference/glob.md` is the specification: every
//! row runs against dirsql as a path-table and as a config `[[table]] glob`.
#![cfg(all(unix, feature = "cli"))]

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use assert_cmd::prelude::*;
use dirsql::{DirSQL, Table, Value};
use tempfile::TempDir;

struct Row {
    pattern: String,
    expected: Vec<String>,
    divergence: String,
}

fn page() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/glob.md");
    fs::read_to_string(path).unwrap()
}

fn after<'a>(page: &'a str, marker: &str) -> &'a str {
    let at = page.find(marker).unwrap_or_else(|| panic!("no {marker}"));
    &page[at + marker.len()..]
}

fn tree_entries(page: &str) -> Vec<String> {
    after(page, "<!-- conformance-tree -->")
        .lines()
        .skip(2)
        .take_while(|line| !line.starts_with("```"))
        .map(str::to_string)
        .collect()
}

fn rows(page: &str) -> Vec<Row> {
    after(page, "<!-- conformance-table -->")
        .lines()
        .skip(1)
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
            let backticked = |cell: &str| -> Vec<String> {
                cell.split('`')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect()
            };
            Row {
                pattern: backticked(cells[0]).remove(0),
                expected: backticked(cells[1]),
                divergence: cells[2].to_string(),
            }
        })
        .collect()
}

fn build(root: &Path, entries: &[String]) {
    for entry in entries {
        let at = root.join(entry.split(" -> ").next().unwrap());
        if let Some((_, target)) = entry.split_once(" -> ") {
            symlink(target, at).unwrap();
        } else if entry.ends_with('/') {
            fs::create_dir_all(at).unwrap();
        } else {
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            fs::write(at, "x\n").unwrap();
        }
    }
    fs::write(root.join(".gitignore"), "*.log\n").unwrap();
}

fn texts(db: &DirSQL, sql: &str, column: &str) -> Vec<String> {
    let mut out: Vec<String> = db
        .query(sql)
        .unwrap()
        .iter()
        .map(|r| match r.get(column) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("{column} was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

fn path_table_paths(root: &Path, pattern: &str) -> Vec<String> {
    let output = std::process::Command::cargo_bin("dirsql")
        .unwrap()
        .args([
            "query",
            &format!("SELECT path FROM '{pattern}'"),
            "--format",
            "json",
        ])
        .current_dir(root)
        .env("HOME", root)
        .output()
        .unwrap();
    if !output.status.success() {
        return vec![String::from_utf8_lossy(&output.stderr).trim().to_string()];
    }
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let mut out: Vec<String> = rows
        .iter()
        .map(|row| row["path"].as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

fn config_paths(root: &Path, glob: &str) -> Vec<String> {
    let base = root.to_path_buf();
    let table = Table::new("t", "CREATE TABLE t (p TEXT)", glob, move |path| {
        let rel = Path::new(path)
            .strip_prefix(&base)
            .unwrap_or(Path::new(path));
        vec![HashMap::from([(
            "p".to_string(),
            Value::Text(rel.to_string_lossy().into_owned()),
        )])]
    });
    let db = match DirSQL::builder().root(root).table(table).build() {
        Ok(db) => db,
        Err(err) => return vec![err.to_string()],
    };
    texts(&db, "SELECT p FROM t", "p")
}

#[test]
fn every_row_of_the_glob_page_holds() {
    let page = page();
    let base = TempDir::new().unwrap();
    let root = base.path().join("proj");
    fs::create_dir(&root).unwrap();
    build(&root, &tree_entries(&page));
    let (root_text, base_text) = (
        root.to_string_lossy().into_owned(),
        base.path().to_string_lossy().into_owned(),
    );

    let mut failures = Vec::new();
    for row in rows(&page) {
        let pattern = row.pattern.replace("$ROOT", &root_text);
        let mut expected: Vec<String> = row
            .expected
            .iter()
            .filter(|cell| *cell != "none")
            .map(|cell| {
                cell.replace("$ROOT", &root_text)
                    .replace("$BASE", &base_text)
            })
            .collect();
        expected.sort();

        let got = path_table_paths(&root, &pattern);
        if got != expected {
            failures.push(format!(
                "path-table {}: {got:?} != {expected:?}",
                row.pattern
            ));
        }
        if row.divergence != "absolute" {
            let glob = pattern.strip_prefix("./").unwrap();
            let got = config_paths(&root, glob);
            // A config table sees each file once, by its own path.
            let expected: Vec<String> = expected.iter().map(|p| p.replace("//", "/")).collect();
            if got != expected {
                failures.push(format!("config {}: {got:?} != {expected:?}", row.pattern));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
