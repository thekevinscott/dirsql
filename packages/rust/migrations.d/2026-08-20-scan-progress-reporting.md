### The startup scan reports progress on stderr (#957)

#### Summary

Building an index — from the CLI or from any SDK — now writes a progress
counter to stderr while the directory walk and the ingest pass run, and one
summary line when each finishes. Nothing about the API changes; what changes is
that a process that used to write nothing to stderr on a successful build may
now write to it. The behavior is gated on stderr being a terminal *and* the
phase running longer than half a second, so a piped, redirected or fast run is
unaffected.

#### Required changes

_None._ No signature changed and no configuration is required. The
`DIRSQL_PROGRESS` environment variable is the opt-out for a program that
insists on an empty stderr.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **stderr may carry progress lines on a terminal.** A build whose walk or
  ingest phase runs longer than 500 ms and whose stderr is a terminal draws
  `dirsql: scanning <n> files` / `dirsql: indexing <n>/<total> files (<p>%)`,
  rewritten in place, then erases the line and prints
  `dirsql: scanned <n> files in <t>` / `dirsql: indexed <n> files in <t>`.
  Set `DIRSQL_PROGRESS=never` to restore an unconditionally empty stderr.
- **Non-terminal stderr is unchanged.** A pipe, a file, a log collector or a
  CI runner gets nothing, at any duration — the default mode consults
  `stderr.is_terminal()` before writing a byte. Anything asserting on captured
  stderr keeps passing.
- **stdout is unchanged**, on every path. Progress never shares the stream the
  query result is printed on.
- **`dirsql::scanner::scan_directory` is unchanged.** The reporting walk is a
  new sibling, `scan_directory_reporting(root, matcher, &mut on_file)`, which
  the existing two-argument function now delegates to with a no-op callback.
  Direct callers (benches, in-workspace tools) compile as before.

#### Verification

```bash
# A slow scan on a terminal: the counter is drawn, erased, and summarized.
dirsql query "SELECT count(*) FROM items" -c ./.dirsql.toml
# dirsql: indexing 40/41 files (97%)      (rewritten in place, then erased)
# dirsql: indexed 41 files in 2.2s
# [{"count(*)":41}]

# Piped: stderr is empty, exactly as before.
dirsql query "SELECT count(*) FROM items" -c ./.dirsql.toml 2>err.txt | jq .
wc -c err.txt   # -> 0

# Forced on without a terminal, for watching a redirected run.
DIRSQL_PROGRESS=always dirsql query "SELECT 1" -c ./.dirsql.toml 2>&1 >/dev/null | tail -1
# -> dirsql: indexed 41 files in 2.2s

# Opted out, even on a terminal.
DIRSQL_PROGRESS=never dirsql query "SELECT count(*) FROM items" -c ./.dirsql.toml
# -> [{"count(*)":41}]      (nothing on stderr)
```
