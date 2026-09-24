### Result columns follow the projection order

**Summary**

Query results are rendered in the order the SELECT list names the columns,
where they were previously alphabetical. This affects the Rust core's CLI
(`--format table` and `--format json`) and the HTTP `POST /query` body. No
signature changed; `DirSQL::query` still returns `Vec<Row>`, and the new
`DirSQL::query_ordered` is additive.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Surface | Before | After |
|---|---|---|
| `dirsql "SELECT path, ctime FROM './'" --format table` | header `ctime  path` | header `path  ctime` |
| `dirsql "SELECT path, ctime FROM './'" --format json` | `{"ctime":…,"path":…}` | `{"path":…,"ctime":…}` |
| `POST /query` response rows | keys sorted alphabetically | keys in projection order |

A consumer that relied on alphabetical key order (e.g. comparing serialized
JSON byte-for-byte) must key off the column names instead.

**Verification**

```bash
dirsql "SELECT path, ctime FROM './' LIMIT 2" --format json
# [{"path":"a.md","ctime":1790181167},…]  -- path first, as written
```
