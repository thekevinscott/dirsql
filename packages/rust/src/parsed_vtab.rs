//! A path-table whose columns come from a parser command's row objects.
//!
//! `CREATE VIRTUAL TABLE t USING dirsql_parsed('<root>', '<glob>', '<parser>')`
//!
//! Unlike the stat path-table ([`crate::vtab`]), whose seven columns are known
//! in advance, this table's columns are whatever the parser emitted. A vtab
//! must declare its schema before any row can flow, so the work happens at
//! `CREATE`: every matched file is parsed, the rows are inferred over
//! ([`crate::infer`]), the schema is declared, and those same rows are then
//! served. The sample and the result are the same rows, so the declared schema
//! always describes the data exactly.
//!
//! The trade-off against the stat path-table is deliberate: reads there are
//! live because the scan happens per statement, whereas here the rows are
//! materialized once. Re-parsing every file on every statement would make a
//! join over a parsed table quadratic in parser invocations, and re-inferring
//! could change the schema out from under a prepared statement.
//!

use std::ffi::c_int;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::vtab::Context;
use rusqlite::{Connection, Error, Result};

use crate::Value;
use crate::infer::{JsonRow, cell, declared_schema, infer_schema};
use crate::matcher::TableMatcher;
use crate::on_file;
use crate::scanner::{PathGlob, scan_glob_checking_root};
use crate::vtab_scaffold::{self, StatementScope, TableSource};

/// SQL module name a parsed path-table is created with.
pub const MODULE_NAME: &str = "dirsql_parsed";

/// Register the parsed path-table module on `conn`.
pub fn load_module(conn: &Connection, scope: Arc<StatementScope>) -> Result<()> {
    vtab_scaffold::load_module::<ParsedTable>(conn, scope)
}

/// Number of module arguments that are not ignore patterns.
const FIXED_ARGS: usize = 5;

/// The module's own arguments: scan root, glob pattern, parser command, the
/// gitignore switch, the index root the parser is spawned in, and the skip
/// rules the scan applies (mirroring the stat path-table's ignore args).
struct ModuleArgs {
    root: PathBuf,
    pattern: String,
    glob: PathGlob,
    command: String,
    gitignore: bool,
    check_root: bool,
    index_root: PathBuf,
    ignore: TableMatcher,
}

/// Parse a parsed path-table's `CREATE VIRTUAL TABLE` arguments. `args[0..3]`
/// are the module, database and table names; the module's own follow — root,
/// glob, parser, the gitignore switch, the index root, then any ignore
/// patterns.
fn parse_module_args(args: &[&[u8]]) -> Result<ModuleArgs> {
    let user_args = vtab_scaffold::user_args(args);

    let [root, pattern, command, gitignore, index_root, ignore @ ..] = user_args.as_slice() else {
        return Err(vtab_scaffold::arity_error(
            MODULE_NAME,
            FIXED_ARGS,
            "root, glob, parser, gitignore switch, index root",
            user_args.len(),
        ));
    };

    Ok(ModuleArgs {
        root: PathBuf::from(root),
        pattern: pattern.clone(),
        glob: vtab_scaffold::compile_glob(pattern)?,
        command: command.clone(),
        gitignore: vtab_scaffold::parse_gitignore(gitignore)?,
        check_root: vtab_scaffold::checks_root(gitignore),
        index_root: PathBuf::from(index_root),
        ignore: vtab_scaffold::compile_ignore(ignore)?,
    })
}

/// Run the parser over every matched file and concatenate the rows.
///
/// `run` is injected so the fan-out and the ordering can be unit-tested without
/// spawning a process. Production passes a closure over [`run_parser`].
///
/// One run over every file, matching the `on-file` hook contract: the
/// parser's output is the table's rows, so a run that fails or does not parse
/// is the table's error and the schema is inferred from what it emitted.
fn collect_rows(rel_paths: &[PathBuf], run: &RunParser<'_>) -> Result<Vec<JsonRow>> {
    run(rel_paths).map_err(Error::ModuleError)
}

/// The parser over a table's files: every root-relative path in, the table's
/// rows out, or the message to fail the table with.
type RunParser<'a> = dyn Fn(&[PathBuf]) -> std::result::Result<Vec<JsonRow>, String> + 'a;

/// The error raised when the sample yields nothing to infer from. SQLite has
/// no zero-column table, and inventing a placeholder column would make
/// `SELECT *` mean something the parser never said.
fn no_rows_message(pattern: &str) -> String {
    format!("{MODULE_NAME}: parser produced no rows for `{pattern}`; cannot infer a schema")
}

/// Run the parser once over `rel_paths`, handing it their absolute paths as
/// trailing arguments, from the index root as its working directory, with
/// `{root}` naming it, matching the `on-file` contract.
fn run_parser(
    command: &str,
    index_root: &Path,
    root: &Path,
    rel_paths: &[PathBuf],
) -> std::result::Result<Vec<JsonRow>, String> {
    let abs_paths: Vec<PathBuf> = rel_paths.iter().map(|rel| root.join(rel)).collect();
    on_file::run(command, index_root, index_root, &abs_paths)
}

/// Column name for an index, or `None` when SQLite asks for one out of range.
fn column_name(names: &[String], i: c_int) -> Option<&str> {
    let index = usize::try_from(i).ok()?;
    names.get(index).map(String::as_str)
}

/// The table a `CREATE` produced: the inferred column names and the rows the
/// parsers emitted, both fixed for the life of the table.
struct ParsedTable {
    /// Only the names survive registration: the types live in the schema
    /// SQLite already holds, and a cursor addresses columns by index.
    column_names: Vec<String>,
    rows: Arc<Vec<JsonRow>>,
}

impl TableSource for ParsedTable {
    type Row = JsonRow;

    const NAME: &'static str = MODULE_NAME;

    fn connect(args: &[&[u8]]) -> Result<(String, Self)> {
        let ModuleArgs {
            root,
            pattern,
            glob,
            command,
            gitignore,
            check_root,
            index_root,
            ignore,
        } = parse_module_args(args)?;

        // A parsed path-table honors the same skip rules a stat path-table does
        // (node_modules/.git, gitignore, plus any configured ignore), so a
        // parsed `SELECT * FROM './'` doesn't drown in dependency trees.
        let rel_paths = scan_glob_checking_root(&root, &glob, &ignore, gitignore, check_root);
        let run = |rel: &[PathBuf]| run_parser(&command, &index_root, &root, rel);
        let rows = collect_rows(&rel_paths, &run)?;

        let columns = infer_schema(&rows);
        if columns.is_empty() {
            return Err(Error::ModuleError(no_rows_message(&pattern)));
        }

        let schema = declared_schema(&columns);
        let table = Self {
            column_names: columns.into_iter().map(|c| c.name).collect(),
            rows: Arc::new(rows),
        };
        Ok((schema, table))
    }

    fn rows(&self) -> Arc<Vec<JsonRow>> {
        // Materialized once at CREATE, so every statement reads the same rows
        // rather than re-spawning a parser per file.
        Arc::clone(&self.rows)
    }

    fn column(&self, row: &JsonRow, ctx: &mut Context, i: c_int) -> Result<()> {
        let value = column_name(&self.column_names, i)
            .map(|name| cell(row, name))
            .unwrap_or(Value::Null);
        ctx.set_result(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real vtab behavior over a real directory and a real parser process is
    // covered by `tests/schema_inference.rs` (unit-lint isolation); only the
    // pure helpers are tested here.

    #[test]
    fn module_name_is_stable() {
        assert_eq!(MODULE_NAME, "dirsql_parsed");
    }

    /// The three fixed arguments SQLite prepends before the module's own.
    fn args_with<'a>(user: &[&'a [u8]]) -> Vec<&'a [u8]> {
        let mut all: Vec<&'a [u8]> = vec![
            b"dirsql_parsed".as_slice(),
            b"main".as_slice(),
            b"t".as_slice(),
        ];
        all.extend_from_slice(user);
        all
    }

    #[test]
    fn parse_module_args_extracts_root_glob_command_and_index_root() {
        let args = args_with(&[
            b"'/tmp/notes/docs'",
            b"'**/*.json'",
            b"'cat'",
            b"'gitignore'",
            b"'/tmp/notes'",
        ]);
        let parsed = parse_module_args(&args).unwrap();

        assert_eq!(parsed.root, PathBuf::from("/tmp/notes/docs"));
        assert_eq!(parsed.pattern, "**/*.json");
        assert_eq!(parsed.command, "cat");
        assert_eq!(parsed.index_root, PathBuf::from("/tmp/notes"));
        assert!(parsed.glob.is_match(Path::new("a.json")));
        assert!(!parsed.glob.is_match(Path::new("a.md")));
    }

    #[test]
    fn parse_module_args_accepts_unquoted_arguments() {
        let args = args_with(&[b"/tmp/notes", b"**/*", b"cat", b"gitignore", b"/tmp"]);
        assert_eq!(
            parse_module_args(&args).unwrap().root,
            PathBuf::from("/tmp/notes")
        );
    }

    #[test]
    fn parse_module_args_reads_the_gitignore_switch() {
        let on = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'", b"'/tmp'"]);
        assert!(parse_module_args(&on).unwrap().gitignore);

        let off = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'no-gitignore'", b"'/tmp'"]);
        assert!(!parse_module_args(&off).unwrap().gitignore);
    }

    #[test]
    fn parse_module_args_rejects_an_unknown_gitignore_switch() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'sometimes'", b"'/tmp'"]);
        let err = match parse_module_args(&args) {
            Err(err) => err,
            Ok(_) => panic!("an unknown switch must be rejected"),
        };
        assert!(err.to_string().contains("no-gitignore"), "got: {err}");
    }

    #[test]
    fn parse_module_args_compiles_trailing_ignore_patterns() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"'/tmp'",
            b"'node_modules/**'",
        ]);
        let parsed = parse_module_args(&args).unwrap();
        assert!(parsed.ignore.is_ignored(Path::new("node_modules/pkg/a.js")));
        assert!(!parsed.ignore.is_ignored(Path::new("docs/a.md")));
    }

    #[test]
    fn parse_module_args_with_no_ignore_patterns_ignores_nothing() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'", b"'/tmp'"]);
        let parsed = parse_module_args(&args).unwrap();
        assert!(!parsed.ignore.is_ignored(Path::new("node_modules/pkg/a.js")));
    }

    #[test]
    fn parse_module_args_rejects_too_few_arguments() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'"]);
        let err = match parse_module_args(&args) {
            Err(err) => err,
            Ok(_) => panic!("four arguments must be rejected"),
        };
        assert!(
            err.to_string().contains("at least"),
            "error should name the arity, got: {err}"
        );
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_glob() {
        let args = args_with(&[b"'/tmp'", b"'[z-a]'", b"'cat'", b"'gitignore'", b"'/tmp'"]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_ignore_pattern() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"'/tmp'",
            b"'['",
        ]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn collect_rows_hands_every_file_to_one_run_in_scan_order() {
        let paths = vec![PathBuf::from("a.json"), PathBuf::from("b.json")];
        let seen = std::cell::RefCell::new(Vec::new());
        let rows = collect_rows(&paths, &|rel| {
            seen.borrow_mut().push(rel.to_vec());
            Ok(vec![
                JsonRow(vec![("i".into(), serde_json::json!(1))]),
                JsonRow(vec![("i".into(), serde_json::json!(2))]),
            ])
        })
        .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(
            *seen.borrow(),
            vec![paths.clone()],
            "one run over all files"
        );
    }

    #[test]
    fn collect_rows_over_no_files_hands_the_run_no_paths() {
        let rows = collect_rows(&[], &|rel| {
            assert!(rel.is_empty());
            Ok(Vec::new())
        })
        .unwrap();
        assert_eq!(rows, Vec::new());
    }

    #[test]
    fn collect_rows_surfaces_a_run_failure_as_the_tables_error() {
        let paths = vec![PathBuf::from("a.json")];
        let err = collect_rows(&paths, &|_| Err("exit 7".to_string())).unwrap_err();
        assert!(
            matches!(err, Error::ModuleError(ref m) if m == "exit 7"),
            "got: {err}"
        );
    }

    #[test]
    fn no_rows_message_names_the_glob_and_says_no_rows() {
        let message = no_rows_message("**/*.json");
        assert!(message.contains("no rows"), "got: {message}");
        assert!(message.contains("**/*.json"), "got: {message}");
    }

    fn names() -> Vec<String> {
        vec!["a".to_string(), "b".to_string()]
    }

    #[test]
    fn column_name_resolves_a_declared_index() {
        assert_eq!(column_name(&names(), 0), Some("a"));
        assert_eq!(column_name(&names(), 1), Some("b"));
    }

    #[test]
    fn column_name_is_none_past_the_last_column() {
        assert_eq!(column_name(&names(), 2), None);
    }

    #[test]
    fn column_name_is_none_for_a_negative_index() {
        assert_eq!(column_name(&names(), -1), None);
    }

    // The run_parser tests spawn a real `sh`. Their test code statically
    // references only `super::` items (plus pure std), matching the pattern
    // `command.rs` uses for the runner underneath.

    fn ids(rows: &[JsonRow]) -> Vec<i64> {
        rows.iter()
            .map(|r| r.get("id").unwrap().as_i64().unwrap())
            .collect()
    }

    #[test]
    fn run_parser_returns_the_commands_rows() {
        let rows = run_parser(
            r#"sh -c "printf '{\"id\":1}'""#,
            Path::new("."),
            Path::new("."),
            &[PathBuf::from("a.json")],
        )
        .unwrap();
        assert_eq!(ids(&rows), vec![1]);
    }

    /// `{root}` and every absolute path reach the command as arguments, in
    /// order. The command writes them to a side file rather than into the
    /// JSON payload, where a Windows path's `\` would need escaping.
    #[test]
    fn run_parser_appends_every_absolute_path_after_the_index_root() {
        let index_root =
            std::env::temp_dir().join(format!("dirsql-run-parser-{}", std::process::id()));
        let root = index_root.join("docs");
        std::fs::create_dir_all(&root).unwrap();
        run_parser(
            r#"sh -c 'printf "%s\n%s\n%s\n" "$1" "$2" "$3" > seen' sh {root}"#,
            &index_root,
            &root,
            &[PathBuf::from("a.json"), PathBuf::from("b.json")],
        )
        .unwrap();
        let seen = std::fs::read_to_string(index_root.join("seen")).unwrap();
        std::fs::remove_dir_all(&index_root).unwrap();
        assert_eq!(
            seen,
            format!(
                "{}\n{}\n{}\n",
                index_root.display(),
                root.join("a.json").display(),
                root.join("b.json").display()
            ),
            "the side file lands in the index root, which is the working directory"
        );
    }

    #[test]
    fn run_parser_surfaces_a_command_failure_as_a_message() {
        let err = run_parser(
            "sh -c 'exit 7'",
            Path::new("."),
            Path::new("."),
            &[PathBuf::from("a.json")],
        )
        .unwrap_err();
        assert!(err.contains('7'), "the exit code is reported, got: {err}");
    }
}
