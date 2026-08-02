## Summary

A per-file failure during the initial scan is no longer fatal (#714). A hook
that fails, or that returns a row the table rejects, costs that file alone: the
scan indexes everything else, commits, and records the failure. Previously a
strict-mode violation aborted the scan outright — one bad column cost every
other file's rows — while a non-zero hook exit was already silently skipped. The
two are now consistent.

The visible consequence is an exit code. A run that skipped files exits **23**
(rsync's "partial transfer due to error") instead of `0`, so a caller can tell a
partial index from a complete one. That distinction did not exist before, which
is why it is a breaking change for scripts rather than a pure improvement.

## Required changes

**`DirSqlError::OnFile` and `DirSqlError::OnFileMany` are removed.** Nothing can
produce them now. An exhaustive `match` on `DirSqlError` drops those arms:

```rust
// before
match err {
    DirSqlError::OnFile { path, message } => report(path, message),
    DirSqlError::OnFileMany { failures } => failures.iter().for_each(report_one),
    other => return Err(other),
}

// after — the build no longer fails for this reason; ask the database instead
let db = DirSQL::new(root, tables)?;
for failure in db.scan_failures() {
    report(&failure.path, &failure.message);
}
```

**A script that treats any non-zero exit as failure now sees `23`.** If a
partial index should be acceptable, accept it explicitly:

```sh
# before: relied on `dirsql` exiting 0 while quietly skipping files
dirsql "SELECT * FROM docs" | jq .

# after: accept a partial index, still failing on a real error
dirsql "SELECT * FROM docs" | jq . || [ $? -eq 23 ]
```

Under `set -e` and *without* that guard, a scan with skipped files now stops the
script — which is the point: it previously continued against a silently
incomplete index.

## Deprecations removed

_None._

## Behavior changes without code changes

**A strict-mode violation skips its file instead of aborting the scan.** A
`strict` table whose hook returns an unexpected or missing column used to fail
the build; it now yields a skipped file and a built database containing every
other file's rows. The rejected row is still never inserted, and the column name
still never reaches SQL — only the blast radius changed.

**Skipped files are reported by the CLI, capped.** At most ten are named on
stderr, followed by `... and N more`. Previously `run_on_file` printed one line
per failing file with no cap, and printed nothing at all for strict violations
(they aborted instead). Paths in these lines are now root-relative, where the
old `run_on_file` line printed the absolute path.

**A library consumer no longer gets stderr noise from the core.** `run_on_file`
returned `Vec<Row>` and logged its own failures; it now returns `Result` and the
scan records them. Read `DirSQL::scan_failures()` instead of scraping stderr.

**The persistent cache commits partially.** A scan with failures used to roll
back entirely; it now commits the files that parsed. The failed file never
reaches the file index, so the next scan treats it as unknown and retries it —
the cache is incomplete, never wrong. A caller that relied on "a failed build
leaves the cache untouched" no longer gets that.

**Watch-mode gains an error event it did not emit.** With `run_on_file` now
returning `Result`, a command hook that fails during live watching produces an
`error` row event, where it previously produced silence.

## Verification

```sh
# a directory with one good file and two whose on-file hook exits non-zero
dirsql "SELECT name FROM items" -c .dirsql.toml; echo "EXIT=$?"
```

Expected: the good file's rows on stdout as JSON, one `dirsql: skipping …` line
per failed file on stderr, and `EXIT=23`. Before this change the same command
printed the rows and `EXIT=0`.

```sh
# a `strict = true` table where one file's hook emits an unexpected column
dirsql "SELECT name FROM items" -c strict.toml; echo "EXIT=$?"
```

Expected: every well-formed file's rows on stdout, one skip line naming the bad
file, and `EXIT=23`. Before this change: no rows at all, a
`Schema mismatch: extra columns` error on stderr, and `EXIT=1`.
