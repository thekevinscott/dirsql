use std::ffi::c_int;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use globset::GlobSet;
use rusqlite::vtab::Context;
use rusqlite::{Connection, Result};

use crate::compute_stat_virtuals;
use crate::matcher::TableMatcher;
use crate::path_table;
use crate::scanner::scan_glob;
use crate::vtab_scaffold::{self, TableSource};
use crate::{Row, Value};

/// SQL module name a path-table is created with:
/// `CREATE VIRTUAL TABLE t USING dirsql_path('<root>', '<glob>', '<path prefix>',
/// '<gitignore|no-gitignore>'[, '<ignore>'...])`.
pub const MODULE_NAME: &str = "dirsql_path";

/// Number of module arguments that are not ignore patterns.
const FIXED_ARGS: usize = 4;

/// The seven stat columns, in declaration order.
pub const STAT_COLUMNS: [&str; 7] = ["path", "basename", "dir", "ext", "size", "mtime", "ctime"];

/// Column index of the lazily-read `content`, which follows the stat columns.
const CONTENT_COLUMN: usize = STAT_COLUMNS.len();

/// Schema a path-table declares to SQLite.
///
/// `content` is declared last and `HIDDEN` so SQLite excludes it from
/// `SELECT *` while still resolving it by name. That is what makes laziness
/// structural rather than aspirational: a bare `SELECT *` never asks for the
/// column, so the file body is never read.
pub fn declared_schema() -> String {
    let stats = STAT_COLUMNS
        .iter()
        .map(|c| format!("{c} {}", column_type(c)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CREATE TABLE x({stats}, content TEXT HIDDEN)")
}

/// Declared SQLite type for a stat column.
fn column_type(column: &str) -> &'static str {
    match column {
        "size" | "mtime" | "ctime" => "INTEGER",
        _ => "TEXT",
    }
}

/// Read `path` as text, yielding `None` when it is unreadable or not valid
/// UTF-8. A file that cannot be read is a NULL cell, never a failed row: the
/// filesystem is allowed to be messy and a query over it should still return.
fn read_text(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}

/// Everything a scan needs: where to walk, what to match, what to call the
/// results, and what to skip.
struct ScanSpec {
    root: PathBuf,
    glob: GlobSet,
    /// Prepended to each matched path before the stat columns are computed.
    /// Empty for index-root-relative tables.
    path_prefix: PathBuf,
    ignore: TableMatcher,
    /// Literal directories the pattern named outright; skip rules are judged
    /// below this.
    ignore_base: PathBuf,
    /// Whether the scan respects `.gitignore` files (off under `--no-ignore`).
    gitignore: bool,
}

/// One matched file: its absolute path, kept for the lazy content read, and
/// its already-computed stat columns.
struct FileRow {
    abs_path: PathBuf,
    stats: Row,
}

/// Parse a path-table's own `CREATE VIRTUAL TABLE` arguments into its scan
/// spec. `args[0..3]` are the module, database and table names; the module's
/// own arguments follow — root, glob, path prefix, the gitignore switch, then
/// any ignore patterns.
fn parse_module_args(args: &[&[u8]]) -> Result<ScanSpec> {
    let user_args = vtab_scaffold::user_args(args);

    let [root, pattern, path_prefix, gitignore, ignore @ ..] = user_args.as_slice() else {
        return Err(vtab_scaffold::arity_error(
            MODULE_NAME,
            FIXED_ARGS,
            "root, glob, path prefix, gitignore switch",
            user_args.len(),
        ));
    };

    Ok(ScanSpec {
        root: PathBuf::from(root),
        glob: vtab_scaffold::compile_glob(pattern)?,
        path_prefix: PathBuf::from(path_prefix),
        ignore: vtab_scaffold::compile_ignore(ignore)?,
        ignore_base: path_table::ignore_base(pattern),
        gitignore: vtab_scaffold::parse_gitignore(gitignore)?,
    })
}

/// The string a matched file is reported under: the relative path as scanned,
/// under the table's path prefix when it has one.
fn reported_path(path_prefix: &Path, rel_path: &Path) -> String {
    if path_prefix.as_os_str().is_empty() {
        return rel_path.to_string_lossy().into_owned();
    }
    path_prefix.join(rel_path).to_string_lossy().into_owned()
}

/// Whether `column` addresses the hidden `content` column.
fn is_content_column(column: usize) -> bool {
    column == CONTENT_COLUMN
}

/// Value of a stat column, NULL when the index is out of range or the fact was
/// unavailable for this file (an unstattable file still yields a row).
fn stat_cell(stats: &Row, column: usize) -> Value {
    STAT_COLUMNS
        .get(column)
        .and_then(|name| stats.get(*name))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Build the row set for a scan. `stat` is injected so unit tests can supply
/// deterministic facts without touching the filesystem; production passes
/// [`compute_stat_virtuals`].
fn build_rows(
    root: &Path,
    path_prefix: &Path,
    rel_paths: Vec<PathBuf>,
    stat: &dyn Fn(&str, &Path) -> Row,
) -> Vec<FileRow> {
    rel_paths
        .into_iter()
        .map(|rel_path| {
            let abs_path = root.join(&rel_path);
            let stats = stat(&reported_path(path_prefix, &rel_path), &abs_path);
            FileRow { abs_path, stats }
        })
        .collect()
}

impl TableSource for ScanSpec {
    type Row = FileRow;

    const NAME: &'static str = MODULE_NAME;

    fn connect(args: &[&[u8]]) -> Result<(String, Self)> {
        Ok((declared_schema(), parse_module_args(args)?))
    }

    fn rows(&self) -> Arc<Vec<FileRow>> {
        // The scan runs per statement rather than at CREATE, which is what
        // makes reads live: each statement sees the filesystem as it is now.
        let rel_paths = scan_glob(
            &self.root,
            &self.glob,
            &self.ignore,
            &self.ignore_base,
            self.gitignore,
        );
        Arc::new(build_rows(
            &self.root,
            &self.path_prefix,
            rel_paths,
            &compute_stat_virtuals,
        ))
    }

    fn column(&self, row: &FileRow, ctx: &mut Context, i: c_int) -> Result<()> {
        let column = usize::try_from(i).unwrap_or(usize::MAX);

        if is_content_column(column) {
            // The one effectful read, reached only when a query names the
            // column: this is where laziness actually lives.
            return match read_text(&row.abs_path) {
                Some(text) => ctx.set_result(&Value::Text(text)),
                None => ctx.set_result(&Value::Null),
            };
        }

        ctx.set_result(&stat_cell(&row.stats, column))
    }
}

/// Register the path-table module on `conn`.
pub fn load_module(conn: &Connection) -> Result<()> {
    vtab_scaffold::load_module::<ScanSpec>(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real vtab behavior over a real directory is covered by `tests/vtab.rs`
    // (unit-lint isolation); only the pure helpers are tested here.

    #[test]
    fn stat_columns_are_the_seven_documented_names() {
        assert_eq!(
            STAT_COLUMNS,
            ["path", "basename", "dir", "ext", "size", "mtime", "ctime"]
        );
    }

    #[test]
    fn declared_schema_lists_every_stat_column() {
        let schema = declared_schema();
        for col in STAT_COLUMNS {
            assert!(schema.contains(col), "{col} missing from {schema}");
        }
    }

    #[test]
    fn declared_schema_marks_content_hidden_and_last() {
        let schema = declared_schema();
        assert!(
            schema.contains("content TEXT HIDDEN"),
            "content must be HIDDEN: {schema}"
        );
        assert!(
            schema.find("content").unwrap() > schema.find("ctime").unwrap(),
            "content must follow the stat columns: {schema}"
        );
    }

    #[test]
    fn content_column_index_follows_the_stat_columns() {
        assert_eq!(CONTENT_COLUMN, 7);
    }

    #[test]
    fn numeric_stat_columns_declare_integer() {
        assert_eq!(column_type("size"), "INTEGER");
        assert_eq!(column_type("mtime"), "INTEGER");
        assert_eq!(column_type("ctime"), "INTEGER");
    }

    #[test]
    fn path_like_stat_columns_declare_text() {
        assert_eq!(column_type("path"), "TEXT");
        assert_eq!(column_type("basename"), "TEXT");
        assert_eq!(column_type("dir"), "TEXT");
        assert_eq!(column_type("ext"), "TEXT");
    }

    #[test]
    fn module_name_is_stable() {
        assert_eq!(MODULE_NAME, "dirsql_path");
    }

    /// The three fixed arguments SQLite prepends before the module's own.
    fn args_with<'a>(user: &[&'a [u8]]) -> Vec<&'a [u8]> {
        let mut all: Vec<&'a [u8]> = vec![
            b"dirsql_path".as_slice(),
            b"main".as_slice(),
            b"t".as_slice(),
        ];
        all.extend_from_slice(user);
        all
    }

    #[test]
    fn parse_module_args_extracts_root_and_glob() {
        let args = args_with(&[b"'/tmp/notes'", b"'**/*.md'", b"''", b"'gitignore'"]);
        let spec = parse_module_args(&args).unwrap();

        assert_eq!(spec.root, PathBuf::from("/tmp/notes"));
        assert!(spec.glob.is_match(Path::new("a.md")));
        assert!(!spec.glob.is_match(Path::new("a.csv")));
    }

    #[test]
    fn parse_module_args_accepts_unquoted_arguments() {
        let args = args_with(&[b"/tmp/notes", b"**/*", b"", b"gitignore"]);
        assert_eq!(
            parse_module_args(&args).unwrap().root,
            PathBuf::from("/tmp/notes")
        );
    }

    #[test]
    fn parse_module_args_extracts_the_path_prefix() {
        let args = args_with(&[b"'/var/log'", b"'*.log'", b"'/var/log'", b"'gitignore'"]);
        assert_eq!(
            parse_module_args(&args).unwrap().path_prefix,
            PathBuf::from("/var/log")
        );
    }

    #[test]
    fn parse_module_args_derives_the_ignore_base_from_the_glob() {
        let args = args_with(&[b"'/tmp'", b"'docs/**/*'", b"''", b"'gitignore'"]);
        assert_eq!(
            parse_module_args(&args).unwrap().ignore_base,
            PathBuf::from("docs")
        );
    }

    #[test]
    fn parse_module_args_reads_the_gitignore_switch() {
        let on = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'gitignore'"]);
        assert!(parse_module_args(&on).unwrap().gitignore);

        let off = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'no-gitignore'"]);
        assert!(!parse_module_args(&off).unwrap().gitignore);
    }

    #[test]
    fn parse_module_args_rejects_an_unknown_gitignore_switch() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'sometimes'"]);
        let err = parse_module_args(&args)
            .err()
            .expect("an unknown switch must be rejected");
        assert!(err.to_string().contains("no-gitignore"), "got: {err}");
    }

    #[test]
    fn parse_module_args_compiles_trailing_ignore_patterns() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"''",
            b"'gitignore'",
            b"'node_modules/**'",
        ]);
        let spec = parse_module_args(&args).unwrap();

        assert!(spec.ignore.is_ignored(Path::new("node_modules/a.js")));
        assert!(!spec.ignore.is_ignored(Path::new("docs/a.md")));
    }

    #[test]
    fn parse_module_args_accepts_no_ignore_patterns() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'gitignore'"]);
        let spec = parse_module_args(&args).unwrap();
        assert!(!spec.ignore.is_ignored(Path::new("node_modules/a.js")));
    }

    #[test]
    fn parse_module_args_rejects_too_few_arguments() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''"]);
        let err = parse_module_args(&args)
            .err()
            .expect("arity must be enforced");
        assert!(
            err.to_string().contains("at least 4 arguments"),
            "error should name the arity, got: {err}"
        );
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_glob() {
        let args = args_with(&[b"'/tmp'", b"'['", b"''", b"'gitignore'"]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_ignore_pattern() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'gitignore'", b"'['"]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn reported_path_is_the_relative_path_without_a_prefix() {
        assert_eq!(
            reported_path(Path::new(""), Path::new("docs/a.md")),
            "docs/a.md"
        );
    }

    #[test]
    fn reported_path_is_absolute_under_a_prefix() {
        assert_eq!(
            reported_path(Path::new("/var/log"), Path::new("a.log")),
            "/var/log/a.log"
        );
    }

    #[test]
    fn is_content_column_identifies_only_the_hidden_column() {
        assert!(is_content_column(CONTENT_COLUMN));
        assert!(!is_content_column(0));
        assert!(!is_content_column(CONTENT_COLUMN - 1));
    }

    fn stats_with(name: &str, value: Value) -> Row {
        let mut row = Row::new();
        row.insert(name.to_string(), value);
        row
    }

    #[test]
    fn stat_cell_returns_the_declared_column_value() {
        let stats = stats_with("path", Value::Text("a.md".into()));
        assert_eq!(stat_cell(&stats, 0), Value::Text("a.md".into()));
    }

    #[test]
    fn stat_cell_is_null_when_the_fact_is_absent() {
        let stats = stats_with("path", Value::Text("a.md".into()));
        // `ext` is index 3 and absent for an extensionless file.
        assert_eq!(stat_cell(&stats, 3), Value::Null);
    }

    #[test]
    fn stat_cell_is_null_for_an_out_of_range_column() {
        let stats = stats_with("path", Value::Text("a.md".into()));
        assert_eq!(stat_cell(&stats, 99), Value::Null);
    }

    #[test]
    fn build_rows_joins_each_relative_path_onto_the_root() {
        let rows = build_rows(
            Path::new("/root"),
            Path::new(""),
            vec![PathBuf::from("docs/a.md")],
            &|rel, _abs| stats_with("path", Value::Text(rel.to_string())),
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].abs_path, PathBuf::from("/root/docs/a.md"));
        assert_eq!(
            rows[0].stats.get("path"),
            Some(&Value::Text("docs/a.md".into())),
            "stats are computed from the relative path"
        );
    }

    #[test]
    fn build_rows_preserves_scan_order() {
        let rows = build_rows(
            Path::new("/root"),
            Path::new(""),
            vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
            &|rel, _abs| stats_with("path", Value::Text(rel.to_string())),
        );

        let paths: Vec<_> = rows.iter().map(|r| r.abs_path.clone()).collect();
        assert_eq!(
            paths,
            vec![PathBuf::from("/root/a.md"), PathBuf::from("/root/b.md")]
        );
    }

    #[test]
    fn build_rows_yields_nothing_for_an_empty_scan() {
        let rows = build_rows(
            Path::new("/root"),
            Path::new(""),
            Vec::new(),
            &|_rel, _abs| Row::new(),
        );
        assert!(rows.is_empty());
    }

    #[test]
    fn build_rows_reports_paths_under_the_prefix() {
        let rows = build_rows(
            Path::new("/var/log"),
            Path::new("/var/log"),
            vec![PathBuf::from("a.log")],
            &|rel, _abs| stats_with("path", Value::Text(rel.to_string())),
        );

        assert_eq!(
            rows[0].stats.get("path"),
            Some(&Value::Text("/var/log/a.log".into())),
            "an absolute path-table reports absolute paths"
        );
        assert_eq!(
            rows[0].abs_path,
            PathBuf::from("/var/log/a.log"),
            "content is still read from the scanned file"
        );
    }
}
