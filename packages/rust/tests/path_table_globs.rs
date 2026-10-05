//! Integration tests for path-table *glob semantics*: how the string a user
//! writes where a table name goes becomes a concrete scan. Real filesystem,
//! real SQLite, SDK public API.

use std::fs;
use std::path::Path;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

/// How an absolute path-table reports `path`: always `/`-separated.
fn reported(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// A tree with a nested doc directory, a top-level file, a dotfile, and the
/// two directories a zero-config scan must not drown in.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs/nested")).unwrap();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::create_dir_all(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("skip")).unwrap();

    fs::write(root.path().join("top.md"), "top").unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha").unwrap();
    fs::write(root.path().join("docs/b.md"), "bravo").unwrap();
    fs::write(root.path().join("docs/nested/deep.md"), "deep").unwrap();
    fs::write(root.path().join("node_modules/pkg/index.js"), "js").unwrap();
    fs::write(root.path().join(".git/config"), "cfg").unwrap();
    fs::write(root.path().join("skip/s.md"), "skipped").unwrap();
    root
}

fn open(root: &TempDir) -> DirSQL {
    DirSQL::new(root.path(), vec![]).unwrap()
}

fn texts(rows: &[Row], column: &str) -> Vec<String> {
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| match r.get(column) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("{column} was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

fn paths(db: &DirSQL, sql: &str) -> Vec<String> {
    texts(&db.query(sql).unwrap(), "path")
}

#[test]
fn a_bare_dot_slash_lists_the_index_root_one_level_deep() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './'"),
        vec!["top.md"],
        "'./' is one level, like `ls`: it must not reach docs/"
    );
}

#[test]
fn a_directory_path_lists_one_level() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs'"),
        vec!["docs/a.md", "docs/b.md"],
        "'./docs' is `ls docs`: it must not reach docs/nested"
    );
}

#[test]
fn a_trailing_slash_directory_path_also_lists_one_level() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs/'"),
        vec!["docs/a.md", "docs/b.md"],
    );
}

#[test]
fn an_explicit_star_is_the_same_as_the_bare_dot_slash() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './*'"),
        paths(&db, "SELECT path FROM './'"),
    );
}

#[test]
fn a_double_star_scans_every_depth() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './**'"),
        vec![
            "docs/a.md",
            "docs/b.md",
            "docs/nested/deep.md",
            "skip/s.md",
            "top.md"
        ],
        "'./**' is the recursive spelling"
    );
}

#[test]
fn a_file_created_two_levels_down_does_not_join_a_one_level_table() {
    let root = fixture();
    let db = open(&root);
    assert_eq!(paths(&db, "SELECT path FROM './'"), vec!["top.md"]);

    fs::write(root.path().join("docs/nested/late.md"), "late").unwrap();
    fs::write(root.path().join("late.md"), "late").unwrap();

    assert_eq!(
        paths(&db, "SELECT path FROM './'"),
        vec!["late.md", "top.md"],
        "a live re-scan still stops at one level"
    );
    assert!(
        paths(&db, "SELECT path FROM './**'").contains(&"docs/nested/late.md".to_string()),
        "the recursive spelling sees the deep file"
    );
}

#[test]
fn an_explicit_star_inside_a_directory_is_not_recursive() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs/*'"),
        vec!["docs/a.md", "docs/b.md"],
        "'./docs/*' must not reach docs/nested"
    );
}

#[test]
fn a_single_file_path_is_exactly_one_row() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './docs/a.md'").unwrap();

    assert_eq!(rows.len(), 1, "one file is one row: {rows:?}");
    assert_eq!(rows[0].get("path"), Some(&Value::Text("docs/a.md".into())));
}

#[test]
fn a_glob_metacharacter_path_is_used_as_written() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs/**/*.md'"),
        vec!["docs/a.md", "docs/b.md", "docs/nested/deep.md"],
    );
}

#[test]
fn a_recursive_scan_skips_vcs_and_dependency_directories() {
    let root = fixture();
    let db = open(&root);

    let found = paths(&db, "SELECT path FROM './**'");

    assert!(
        !found.iter().any(|p| p.starts_with("node_modules/")),
        "node_modules must be skipped: {found:?}"
    );
    assert!(
        !found.iter().any(|p| p.starts_with(".git/")),
        ".git must be skipped: {found:?}"
    );
    assert!(
        found.contains(&"top.md".to_string()),
        "ordinary files must survive the skip rules: {found:?}"
    );
}

#[test]
fn naming_a_skipped_directory_explicitly_still_scans_it() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './node_modules/**'"),
        vec!["node_modules/pkg/index.js"],
        "skip rules apply beneath the path you name, not to the path itself"
    );
}

#[test]
fn a_recursive_scan_skips_nested_vcs_and_dependency_directories() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("apps/site/node_modules/pkg")).unwrap();
    fs::create_dir_all(root.path().join("apps/site/.git")).unwrap();
    fs::write(root.path().join("apps/site/main.js"), "js").unwrap();
    fs::write(
        root.path().join("apps/site/node_modules/pkg/index.js"),
        "js",
    )
    .unwrap();
    fs::write(root.path().join("apps/site/.git/config"), "cfg").unwrap();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    let found = paths(&db, "SELECT path FROM './**'");

    assert_eq!(
        found,
        vec!["apps/site/main.js"],
        "node_modules and .git must be skipped at any depth, not only at the root"
    );
}

#[test]
fn a_scoped_directory_scan_also_skips_nested_dependency_directories() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("apps/site/node_modules/pkg")).unwrap();
    fs::write(root.path().join("apps/site/main.js"), "js").unwrap();
    fs::write(
        root.path().join("apps/site/node_modules/pkg/index.js"),
        "js",
    )
    .unwrap();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    assert_eq!(
        paths(&db, "SELECT path FROM './apps/**'"),
        vec!["apps/site/main.js"],
        "the skip rules apply inside a scoped directory scan too"
    );
}

#[test]
fn naming_a_nested_skipped_directory_explicitly_still_scans_it() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("apps/site/node_modules/pkg")).unwrap();
    fs::write(
        root.path().join("apps/site/node_modules/pkg/index.js"),
        "js",
    )
    .unwrap();
    let db = DirSQL::new(root.path(), vec![]).unwrap();

    assert_eq!(
        paths(&db, "SELECT path FROM './apps/site/node_modules/**'"),
        vec!["apps/site/node_modules/pkg/index.js"],
        "skip rules apply beneath the path you name, not to the path itself"
    );
}

#[test]
fn configured_ignore_patterns_apply_to_a_path_table() {
    let root = fixture();
    let db = DirSQL::with_ignore(root.path(), vec![], ["skip/**"]).unwrap();

    let found = paths(&db, "SELECT path FROM './**'");

    assert!(
        !found.iter().any(|p| p.starts_with("skip/")),
        "a configured ignore pattern must apply to path-tables too: {found:?}"
    );
}

#[test]
fn an_absolute_path_table_resolves_and_reports_absolute_paths() {
    let root = fixture();
    let db = open(&root);

    let dir = reported(root.path());
    let found = paths(&db, &format!("SELECT path FROM '{dir}/docs/*.md'"));

    assert_eq!(
        found,
        vec![format!("{dir}/docs/a.md"), format!("{dir}/docs/b.md")],
        "an absolute path-table reports absolute paths"
    );
}

#[test]
fn an_absolute_directory_path_lists_one_level() {
    let root = fixture();
    let db = open(&root);

    let dir = reported(root.path());
    let found = paths(&db, &format!("SELECT path FROM '{dir}/docs'"));

    assert_eq!(
        found,
        vec![format!("{dir}/docs/a.md"), format!("{dir}/docs/b.md")],
        "an absolute directory is one level too"
    );
}

#[test]
fn an_absolute_single_file_path_is_exactly_one_row() {
    let root = fixture();
    let db = open(&root);

    let dir = reported(root.path());
    let rows = db
        .query(&format!("SELECT path FROM '{dir}/docs/a.md'"))
        .unwrap();

    assert_eq!(rows.len(), 1, "one file is one row: {rows:?}");
    assert_eq!(
        rows[0].get("path"),
        Some(&Value::Text(format!("{dir}/docs/a.md")))
    );
}

#[test]
fn a_parent_relative_path_table_resolves_against_the_index_root() {
    let root = fixture();
    let inner = root.path().join("docs/nested");
    let db = DirSQL::new(&inner, vec![]).unwrap();

    let dir = reported(root.path());
    let found = paths(&db, "SELECT path FROM '../*.md'");

    assert_eq!(
        found,
        vec![format!("{dir}/docs/a.md"), format!("{dir}/docs/b.md")],
        "'../' walks up from the index root and reports absolute paths"
    );
}

#[test]
fn an_absolute_path_table_reads_content_from_the_right_file() {
    let root = fixture();
    let db = open(&root);

    let dir = reported(root.path());
    let rows = db
        .query(&format!(
            "SELECT path FROM '{dir}/docs/*.md' WHERE content = 'alpha'"
        ))
        .unwrap();

    assert_eq!(texts(&rows, "path"), vec![format!("{dir}/docs/a.md")]);
}

#[test]
fn a_missing_absolute_path_table_returns_no_rows() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query("SELECT path FROM '/nonexistent-dirsql-dir/*.md'")
        .unwrap();

    assert!(rows.is_empty(), "expected no rows, got {rows:?}");
}

/// Make `dir` traversable but not listable. Returns false when the process
/// bypasses permission checks (root), since the test's premise cannot hold.
#[cfg(unix)]
fn make_unlistable(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o311)).unwrap();
    fs::read_dir(dir).is_err()
}

#[cfg(unix)]
fn make_listable(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn a_prefixed_glob_starts_its_walk_at_the_named_directory() {
    let root = fixture();
    let db = open(&root);
    if !make_unlistable(root.path()) {
        return;
    }

    let found = db.query("SELECT path FROM './docs/*.md'");
    make_listable(root.path());

    assert_eq!(
        texts(&found.unwrap(), "path"),
        vec!["docs/a.md", "docs/b.md"],
        "the walk must begin at docs/, not list the index root"
    );
}

/// A tree with a dotfile and a dot-directory beside ordinary files, the
/// dot-directory holding a nested dot-directory of its own.
fn dotted() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".hidden/.cache")).unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("top.md"), "top").unwrap();
    fs::write(root.path().join(".dotfile"), "dot").unwrap();
    fs::write(root.path().join(".hidden/x.md"), "x").unwrap();
    fs::write(root.path().join(".hidden/.cache/c.md"), "c").unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha").unwrap();
    fs::write(root.path().join("docs/.draft.md"), "draft").unwrap();
    root
}

#[test]
fn a_recursive_scan_hides_dot_named_files_and_directories() {
    let root = dotted();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './**'"),
        vec!["docs/a.md", "top.md"],
        "a dot component the path does not spell is skipped at every depth"
    );
}

#[test]
fn a_one_level_scan_hides_dot_named_files() {
    let root = dotted();
    let db = open(&root);

    assert_eq!(paths(&db, "SELECT path FROM './*'"), vec!["top.md"]);
    assert_eq!(paths(&db, "SELECT path FROM './'"), vec!["top.md"]);
}

#[test]
fn naming_a_dot_directory_lists_beneath_it() {
    let root = dotted();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './.hidden/**'"),
        vec![".hidden/x.md"],
        "the spelled dot directory is scanned; a deeper unspelled one is not"
    );
}

#[test]
fn naming_a_dot_directory_without_a_glob_lists_one_level() {
    let root = dotted();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './.hidden'"),
        vec![".hidden/x.md"]
    );
}

#[test]
fn naming_a_dotfile_lists_exactly_that_file() {
    let root = dotted();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './.dotfile'"),
        vec![".dotfile"]
    );
}

#[test]
fn a_dot_component_spelled_in_the_glob_admits_what_it_matches() {
    let root = dotted();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './**/.draft.md'"),
        vec!["docs/.draft.md"],
        "a spelled dot component is admitted wherever the glob reaches"
    );
    assert_eq!(
        paths(&db, "SELECT path FROM './.hidden/.cache/*'"),
        vec![".hidden/.cache/c.md"]
    );
}

#[test]
fn an_absolute_path_table_hides_dot_named_files_too() {
    let root = dotted();
    let db = open(&root);

    let dir = reported(root.path());
    assert_eq!(
        paths(&db, &format!("SELECT path FROM '{dir}/**'")),
        vec![format!("{dir}/docs/a.md"), format!("{dir}/top.md")]
    );
    assert_eq!(
        paths(&db, &format!("SELECT path FROM '{dir}/.hidden/*'")),
        vec![format!("{dir}/.hidden/x.md")]
    );
}

/// Names no two of which differ only by case, so the fixture holds on a
/// case-insensitive filesystem.
fn posix_class_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("pc")).unwrap();
    for name in ["1.md", "5.md", "A.md", "b.md", "README.MD", "_x.md"] {
        fs::write(root.path().join("pc").join(name), name).unwrap();
    }
    root
}

#[test]
fn a_digit_class_matches_each_digit() {
    let root = posix_class_fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './pc/[[:digit:]].md'"),
        vec!["pc/1.md", "pc/5.md"]
    );
}

#[test]
fn an_upper_class_matches_each_uppercase_letter() {
    let root = posix_class_fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './pc/[[:upper:]]*'"),
        vec!["pc/A.md", "pc/README.MD"]
    );
}

#[test]
fn a_negated_class_matches_everything_outside_it() {
    let root = posix_class_fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './pc/[![:digit:]].md'"),
        vec!["pc/A.md", "pc/b.md"]
    );
}

#[test]
fn a_punct_class_never_matches_the_separator() {
    let root = posix_class_fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './pc[[:punct:]]*.md'"),
        Vec::<String>::new()
    );
    assert_eq!(
        paths(&db, "SELECT path FROM './pc/[[:punct:]]*.md'"),
        vec!["pc/_x.md"]
    );
}
