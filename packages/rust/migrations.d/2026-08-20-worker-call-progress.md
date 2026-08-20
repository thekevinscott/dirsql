### A query reports its worker round trips on stderr (#1001)

#### Summary

`Db::query` now brackets each statement with a progress phase that counts the
round trips its declared `[[dirsql.function]]` workers make, and reports them
on stderr. No API changes; what changes is that a query that used to write
nothing to stderr may now write to it. Gated exactly as the startup scan's
reporting is: terminal-only, and only past a half-second warmup, unless
`DIRSQL_PROGRESS` says otherwise.

#### Required changes

_None._ `DIRSQL_PROGRESS=never` is the opt-out for a program that insists on an
empty stderr, and it already covers the scan's reporting too.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **stderr may carry a worker-call counter on a terminal.** A query whose
  declared functions make round trips for longer than 500 ms draws
  `dirsql: running <n> worker calls`, rewritten in place, then erases it and
  prints `dirsql: ran <n> worker calls in <t>`. A query that calls no worker
  draws nothing, at any setting.
- **Non-terminal stderr is unchanged**, at any duration. Anything asserting on
  captured stderr keeps passing.
- **stdout is unchanged.** The counter is erased before `query()` returns, so
  the result is never printed onto a leftover progress line.
- **`functions::register_all` takes a third argument**, the shared
  `CallReporter`. It is `pub(crate)`, so no crate consumer can be calling it.

#### Verification

```bash
# A worker-backed function over a corpus, forced on so no terminal is needed:
DIRSQL_PROGRESS=always dirsql query "SELECT fake(basename) AS v FROM './*.txt'" -c ./.dirsql.toml
# dirsql: running 13 worker calls      (rewritten in place, then erased)
# dirsql: ran 14 worker calls in 0.7s
# [{"v":"f1.txt"}, …]

# Piped: stderr is empty, and stdout is parseable JSON with nothing glued to it.
dirsql query "SELECT fake(basename) AS v FROM './*.txt'" -c ./.dirsql.toml 2>err.txt | jq .
wc -c err.txt   # -> 0

# A query that calls no worker reports nothing even when forced on:
DIRSQL_PROGRESS=always dirsql query "SELECT 1" -c ./.dirsql.toml
# -> [{"1":1}]      (nothing on stderr)
```
