### Bare `dirsql` opens a REPL instead of exiting 2 (#987)

#### Summary

`dirsql` with no subcommand and no SQL printed a usage error pointing at
`dirsql server` and exited `2`. It now reads SQL statements from stdin until
EOF — a prompted REPL on a terminal, one statement per line from a pipe. No
API changed and no flag was added or removed; the runtime behavior of one
invocation did.

#### Required changes

| Before | After | Fix |
| ------ | ----- | --- |
| `dirsql` (bare, no stdin redirect) — exit 2, usage error on stderr | Opens an interactive REPL and blocks until EOF | Nothing, if a human ran it. A **script** that ran bare `dirsql` expecting an immediate exit-2 must redirect stdin (`dirsql < /dev/null`, which now exits 0) or call the mode it meant (`dirsql query "<sql>"`, `dirsql server`). |
| `echo "SELECT 1" \| dirsql` — exit 2, the SQL ignored | Runs the statement and prints its rows | Nothing. If the exit-2 was being relied on as "this invocation is invalid", pass the SQL as an argument instead: `dirsql "SELECT 1"`. |
| `dirsql --help` / `dirsql --version` | Unchanged | — |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Bare `dirsql` no longer exits at all until stdin reaches EOF. In a
  non-interactive context with an open stdin (a CI step, a supervisor that
  leaves stdin attached to a pipe nothing writes to), the process now waits
  where it used to fail fast.
- The exit code for bare `dirsql` changes from `2` to `0` on a clean EOF —
  **including when statements failed**. Per-statement failures are reported on
  stderr as they happen and do not colour the session's exit code, matching
  interactive `sqlite3`. A script that needs a statement's exit status must use
  `dirsql query "<sql>"`, which still exits 1 on the first failure.
- `PARTIAL_SCAN_EXIT` (23) is not produced by the REPL. Skipped files are still
  named on stderr, once, before the first prompt; the code has no meaning for a
  session whose exit describes the session rather than one scan. `dirsql query`
  keeps it.
- `exit` and `quit` are consumed by the REPL rather than being handed to SQLite.
  Neither was a valid statement before, so nothing that used to run stops
  running.
- A config that fails to load (`-c missing.toml`) exits 1 before the first
  prompt rather than being reported once per statement.

#### Verification

```bash
# Previously: exit 2, "dirsql: no query given…" on stderr.
echo "SELECT 1 AS n" | dirsql
# -> [{"n":1}]
# -> exit 0

# A failure no longer ends the session, and does not colour the exit code.
printf 'SELECT nope FROM missing\nSELECT 2 AS n\n' | dirsql
# -> dirsql: SQLite error: no such table: missing      (stderr)
# -> [{"n":2}]
# -> exit 0

# One-shot mode is unchanged: still exits 1 on the first failure.
dirsql "SELECT nope FROM missing"; echo $?
# -> 1

# Fast-fail for a script that relied on bare `dirsql` returning immediately.
dirsql < /dev/null; echo $?
# -> 0
```
