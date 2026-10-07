//! CLI e2e for path-tables: the real `dirsql` binary, a real temp directory,
//! nothing mocked. Pins the surface a user actually types.

#![cfg(feature = "cli")]

use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha").unwrap();
    fs::write(root.path().join("docs/b.md"), "bravo body").unwrap();
    fs::write(root.path().join("docs/c.csv"), "x,y").unwrap();
    root
}

/// Three depths and a sibling directory, so each spelling's reach is visible.
fn nested() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("folder/sub")).unwrap();
    fs::create_dir_all(root.path().join("sibling")).unwrap();
    fs::write(root.path().join("root.md"), "root").unwrap();
    fs::write(root.path().join("folder/a.md"), "a").unwrap();
    fs::write(root.path().join("folder/sub/b.md"), "b").unwrap();
    fs::write(root.path().join("sibling/c.md"), "c").unwrap();
    root
}

/// How an absolute path-table reports `path`: always `/`-separated.
fn reported(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn run(dir: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

/// Run with `home` as the process home directory, so `~/` resolves somewhere
/// the test owns rather than the developer's real home.
fn run_with_home(dir: &TempDir, home: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .current_dir(dir.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

fn rows(out: &Output) -> Vec<Value> {
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout must be a JSON array")
}

fn basenames(out: &Output) -> Vec<String> {
    let mut names: Vec<String> = rows(out)
        .into_iter()
        .map(|r| r["basename"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

fn paths(out: &Output) -> Vec<String> {
    let mut found: Vec<String> = rows(out)
        .into_iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect();
    found.sort();
    found
}

#[test]
fn a_double_star_returns_stat_rows_for_the_whole_working_directory() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './**'");

    assert_eq!(basenames(&out), vec!["a.md", "b.md", "c.csv"]);
}

#[test]
fn a_bare_dot_slash_lists_the_working_directory_one_level_deep() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './'");

    assert_eq!(paths(&out), vec!["root.md"]);
}

#[test]
fn a_directory_path_lists_one_level() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './folder'");

    assert_eq!(paths(&out), vec!["folder/a.md"]);
}

#[test]
fn an_explicit_star_is_the_same_as_the_bare_dot_slash() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './*'");

    assert_eq!(paths(&out), vec!["root.md"]);
}

#[test]
fn a_double_star_scans_every_depth() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './**'");

    assert_eq!(
        paths(&out),
        vec!["folder/a.md", "folder/sub/b.md", "root.md", "sibling/c.md"]
    );
}

#[test]
fn a_recursive_glob_under_a_directory_is_used_as_written() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './folder/**/*.md'");

    assert_eq!(paths(&out), vec!["folder/a.md", "folder/sub/b.md"]);
}

#[test]
fn a_trailing_slash_after_a_glob_lists_inside_each_matched_directory() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './*/'");

    assert_eq!(
        paths(&out),
        vec!["folder/a.md", "sibling/c.md"],
        "'./*/' is `ls */`: the files directly inside each top-level directory"
    );
}

#[test]
fn a_trailing_slash_after_a_nested_glob_lists_inside_each_matched_directory() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './folder/*/'");

    assert_eq!(paths(&out), vec!["folder/sub/b.md"]);
}

#[test]
fn a_trailing_slash_after_a_double_star_scans_every_depth() {
    let dir = nested();
    let out = run(&dir, "SELECT path FROM './**/'");

    assert_eq!(
        paths(&out),
        vec!["folder/a.md", "folder/sub/b.md", "root.md", "sibling/c.md"]
    );
}

#[test]
fn a_trailing_slash_after_a_home_relative_glob_lists_inside_each_matched_directory() {
    let dir = fixture();
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join("notes")).unwrap();
    fs::write(home.path().join("top.md"), "top").unwrap();
    fs::write(home.path().join("notes/n.md"), "note").unwrap();

    let out = run_with_home(&dir, &home, "SELECT path FROM '~/*/'");

    assert_eq!(
        paths(&out),
        vec![format!("{}/notes/n.md", reported(home.path()))]
    );
}

#[test]
fn a_scoped_glob_limits_the_cli_scan() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs/*.md'");

    assert_eq!(basenames(&out), vec!["a.md", "b.md"]);
}

#[test]
fn two_path_tables_join_against_each_other() {
    let dir = fixture();
    let out = run(
        &dir,
        "SELECT p.basename FROM './docs/*.md' AS p \
         JOIN './**' AS f ON f.path = p.path",
    );

    assert_eq!(basenames(&out), vec!["a.md", "b.md"]);
}

#[test]
fn a_zero_match_path_table_prints_an_empty_array() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs/*.rst'");

    assert_eq!(rows(&out), Vec::<Value>::new());
}

#[test]
fn a_bare_glob_fails_with_the_dot_slash_hint() {
    let dir = fixture();
    let out = run(&dir, "SELECT * FROM '**/*.md'");

    assert!(!out.status.success(), "a bare glob must not succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("did you mean './**/*.md'?"),
        "expected the hint, got: {stderr}"
    );
}

#[test]
fn a_recursive_scan_omits_only_vcs_directories() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("node_modules/pkg/index.js"), "js").unwrap();
    fs::write(dir.path().join(".git/config"), "cfg").unwrap();

    let out = run(&dir, "SELECT path FROM './**'");
    let found = paths(&out);

    assert!(
        found.contains(&"node_modules/pkg/index.js".to_string()),
        "node_modules is walked like any directory: {found:?}"
    );
    assert!(
        !found.iter().any(|p| p.starts_with(".git/")),
        ".git must not drown the scan: {found:?}"
    );
    assert!(
        found.contains(&"docs/a.md".to_string()),
        "ordinary files must survive: {found:?}"
    );
}

#[test]
fn a_single_file_path_returns_exactly_one_row() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs/a.md'");

    assert_eq!(basenames(&out), vec!["a.md"]);
}

#[test]
fn a_home_relative_path_table_resolves_against_the_home_directory() {
    let dir = fixture();
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join("notes")).unwrap();
    fs::write(home.path().join("notes/n.md"), "note").unwrap();

    let out = run_with_home(&dir, &home, "SELECT path, basename FROM '~/notes/*.md'");

    assert_eq!(basenames(&out), vec!["n.md"]);
    let path = rows(&out)[0]["path"].as_str().unwrap().to_string();
    assert_eq!(
        path,
        format!("{}/notes/n.md", reported(home.path())),
        "a '~/' path-table reports absolute paths"
    );
}

#[test]
fn an_absolute_path_table_resolves_outside_the_index_root() {
    let dir = fixture();
    let other = TempDir::new().unwrap();
    fs::write(other.path().join("o.md"), "other").unwrap();

    let out = run(
        &dir,
        &format!("SELECT path FROM '{}/*.md'", other.path().display()),
    );
    let found: Vec<String> = rows(&out)
        .into_iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(found, vec![format!("{}/o.md", reported(other.path()))]);
}

#[test]
fn a_typoed_table_name_fails_without_a_hint() {
    let dir = fixture();
    let out = run(&dir, "SELECT * FROM usrs");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no such table: usrs"),
        "expected the plain SQLite error, got: {stderr}"
    );
    assert!(
        !stderr.contains("did you mean"),
        "a typo must carry no path-table hint, got: {stderr}"
    );
}

#[test]
fn an_unquoted_path_fails_with_the_quoting_hint() {
    let dir = fixture();
    let out = run(&dir, "SELECT * FROM ./");

    assert!(!out.status.success(), "an unquoted path must not succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(r#"did you mean "./"?"#),
        "expected the quoting hint, got: {stderr}"
    );
}

#[test]
fn an_ordinary_syntax_error_carries_no_quoting_hint() {
    let dir = fixture();
    let out = run(&dir, "SELECT * FROM");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("did you mean"),
        "a syntax error with no path in it must stay unhinted, got: {stderr}"
    );
}

/// Ordinary files beside a dotfile and a dot-directory.
fn dotted() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".hidden")).unwrap();
    fs::write(root.path().join("top.md"), "top").unwrap();
    fs::write(root.path().join(".dotfile"), "dot").unwrap();
    fs::write(root.path().join(".hidden/x.md"), "x").unwrap();
    root
}

#[test]
fn a_double_star_hides_dot_named_files_and_directories() {
    let dir = dotted();
    let out = run(&dir, "SELECT path FROM './**'");

    assert_eq!(paths(&out), vec!["top.md"]);
}

#[test]
fn a_single_star_hides_dot_named_files() {
    let dir = dotted();
    let out = run(&dir, "SELECT path FROM './*'");

    assert_eq!(paths(&out), vec!["top.md"]);
}

#[test]
fn naming_a_dot_directory_with_a_glob_lists_beneath_it() {
    let dir = dotted();
    let out = run(&dir, "SELECT path FROM './.hidden/**'");

    assert_eq!(paths(&out), vec![".hidden/x.md"]);
}

#[test]
fn naming_a_dot_directory_lists_one_level() {
    let dir = dotted();
    let out = run(&dir, "SELECT path FROM './.hidden'");

    assert_eq!(paths(&out), vec![".hidden/x.md"]);
}

#[test]
fn naming_a_dotfile_lists_it() {
    let dir = dotted();
    let out = run(&dir, "SELECT path FROM './.dotfile'");

    assert_eq!(paths(&out), vec![".dotfile"]);
}

#[test]
fn a_posix_character_class_matches_like_bash() {
    let root = TempDir::new().unwrap();
    for name in ["1.md", "5.md", "A.md", "b.md"] {
        fs::write(root.path().join(name), name).unwrap();
    }
    let out = run(&root, "SELECT path FROM './[[:digit:]].md'");

    assert_eq!(paths(&out), vec!["1.md", "5.md"]);
}

fn braces() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("br")).unwrap();
    for name in ["q.md", "{q}.md", "a1.md", "a2.md", "a3.md"] {
        fs::write(root.path().join("br").join(name), name).unwrap();
    }
    root
}

#[test]
fn a_brace_group_without_a_comma_matches_literally() {
    let dir = braces();
    let out = run(&dir, "SELECT path FROM './br/{q}.md'");

    assert_eq!(paths(&out), vec!["br/{q}.md"]);
}

#[test]
fn a_brace_sequence_expands_like_bash() {
    let dir = braces();
    let out = run(&dir, "SELECT path FROM './br/a{1..3..2}.md'");

    assert_eq!(paths(&out), vec!["br/a1.md", "br/a3.md"]);
}
