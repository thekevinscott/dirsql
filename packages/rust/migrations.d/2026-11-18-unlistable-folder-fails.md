### A folder that cannot be listed fails config tables and parsed path-tables

#### Summary

A folder a glob reaches that exists but cannot be listed (its open fails, or a read fails part-way) used to be read as empty. It now fails a config table's build and a parsed path-table's query with an error naming the folder; the CLI exits non-zero and the SDKs raise. A missing folder still holds nothing. Three public Rust scanner functions now return a `Result`.

#### Required changes

| Surface | Before | After |
| --- | --- | --- |
| `dirsql::scanner::scan_directory_reporting` | returns `Vec<Found>` | returns `Result<Vec<Found>, UnlistableFolder>` |
| `dirsql::scanner::scan_glob` | returns `Vec<PathBuf>` | returns `Result<Vec<PathBuf>, UnlistableFolder>` |
| `dirsql::scanner::scan_glob_checking_root` | returns `Vec<PathBuf>` | returns `Result<Vec<PathBuf>, UnlistableFolder>` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

`DirSQL::builder().build()` over a config table, and `query` over a parsed path-table, fail with `cannot list folder '<path>': <reason>` where they used to return the rows of every other folder. In the Rust SDK the build error is `DirSqlError::Io` carrying the OS error kind; the query error is a SQLite module error.

#### Verification

```bash
mkdir -p data/open data/locked
echo '{"k":1}' > data/open/a.ndjson
chmod 311 data/locked
dirsql --on-file cat "SELECT * FROM './data/**/*.ndjson'"
```

Expected (as a non-root user): exit status 1 and an error containing `cannot list folder` and `data/locked`.
