### Core: one-shot `dirsql query` no longer times out

#### Summary

The one-shot query path (`dirsql "<sql>"` / `dirsql query "<sql>"`) no longer
borrows the server's 30-second per-query timeout — the query runs to
completion (#819). The process *is* the query, so `timeout(1)` expresses any
bound natively; the baked-in cap only subtracted capability (a first-run
`embed()` query over a few-thousand-file tree was structurally impossible
through the CLI). Server mode is untouched: `POST /query` still enforces
`ServerConfig.query_timeout` (default 30s) and answers `408 Request Timeout`.
Every install channel behaves identically (`pip` / `npm` / `cargo`).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Rust `cli::execute::execute_query` | `execute_query(&state, body, timeout: Duration)` | `execute_query(&state, body, timeout: Option<Duration>)` — pass `Some(bound)` to keep the 408 path, `None` to run unbounded |
| Scripts relying on the implicit 30s cap of `dirsql query` | The CLI killed the query at 30s (`query exceeded 30s timeout`, exit 1) | Wrap the invocation yourself: `timeout 30 dirsql query "<sql>"` (see `timeout(1)`) |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `dirsql query` (and the default `dirsql "<sql>"`): a query that previously
  died at 30 seconds with `dirsql query: query exceeded 30s timeout` now runs
  to completion, however long that takes.
- `dirsql server` / `POST /query`: no change — the 30-second default and the
  `408` classification are exactly as before.

#### Verification

```bash
# One-shot: a deliberately slow query (recursive CTE) outlives 30s and completes.
dirsql query "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 400000000) SELECT COUNT(*) AS n FROM c"
# expected: [{"n":400000000}] after >30s, exit 0 (previously: exit 1 at 30s)

# Server: the 408 path is unchanged.
dirsql server --port 7117 &
curl -s -o /dev/null -w '%{http_code}\n' localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 400000000) SELECT COUNT(*) FROM c"}'
# expected: 408
```
