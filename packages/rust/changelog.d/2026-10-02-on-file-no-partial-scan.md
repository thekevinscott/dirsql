**Removed** The partial-scan exit code `23` and the `dirsql: skipping ...`
diagnostics. An `on-file` command runs once per table, so a run that fails,
prints no output, or prints something other than a JSON array of row objects
is a build error naming the table: the CLI prints it and exits `1`, and the
SDK constructors return it. `scan_failures()` now records only programmatic
per-file hooks. Also removed is the persisted row cache for a path-table
parsed with `--on-file`: the parser runs on every start, and the cache file's
`_dirsql_parsed_rows` table is no longer created or read.
