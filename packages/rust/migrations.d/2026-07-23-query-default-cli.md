### Query is the default; the server moves to `dirsql server` (#662)

#### Summary

The CLI defaults were inverted. Previously bare `dirsql` (no subcommand)
started the HTTP server, and running a query required the explicit
`dirsql query "<sql>"`. For a tool whose headline is `SELECT * FROM './'`,
query is the behavior reached for first, so it is now the **default**:
`dirsql "<sql>"` runs one query and prints JSON rows, identical to
`dirsql query "<sql>"`. The server moved behind a new `dirsql server`
subcommand, and its `--host`/`--port`/`--persist` options became
`server`-local flags rather than top-level "used when no subcommand" options.
`dirsql query "<sql>"` is retained as an explicit synonym and `dirsql init`
is unchanged. Affects the `dirsql` CLI shipped in the Rust crate and in the
`pip` / `npm` launchers; no SDK signature changes.

#### Required changes

| Surface | Before | After |
| --- | --- | --- |
| Run a query | `dirsql query "SELECT * FROM './'"` | `dirsql "SELECT * FROM './'"` (or keep `dirsql query …`) |
| Query with a config | `dirsql query "…" -c ./.dirsql.toml` | unchanged (or `dirsql "…" -c ./.dirsql.toml`) |
| Start the server | `dirsql` | `dirsql server` |
| Server bind flags | `dirsql --host 0.0.0.0 --port 9000` | `dirsql server --host 0.0.0.0 --port 9000` |
| Server persistence | `dirsql --persist` | `dirsql server --persist` |
| `dirsql init` | `dirsql init` | unchanged |

#### Deprecations removed

_None._ `dirsql query` is retained as an explicit synonym for the new default.

#### Behavior changes without code changes

- Bare `dirsql` no longer starts a server. With SQL (`dirsql "<sql>"`) it runs
  a one-shot query; with no arguments it prints a usage error pointing at
  `dirsql server` and exits `2` (it does **not** silently start the server).
- `--host`/`--port`/`--persist` are rejected at the top level; pass them after
  the `server` subcommand.

#### Verification

In a directory containing some files:

```console
$ dirsql "SELECT basename FROM './' ORDER BY basename LIMIT 1"
[{"basename":"a.txt"}]

$ dirsql
dirsql: no query given. Run a query with `dirsql "SELECT * FROM './'"`, or start the HTTP server with `dirsql server`. See `dirsql --help`.
$ echo $?
2

$ dirsql server --port 7117
Running at localhost:7117
```
