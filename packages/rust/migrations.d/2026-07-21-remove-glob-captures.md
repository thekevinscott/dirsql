### Glob captures removed; colliding placeholder + column is a load-time error (#655)

#### Summary

Glob `{name}` placeholders no longer populate DDL columns. Previously a
placeholder captured the matched path segment and injected it as a column of
the same name; that capture-extraction path is deleted. `{name}` remains valid
**match** syntax and behaves exactly like `*` (matching is unchanged — the
matcher already rewrote `{name}` to `*` before compiling the glob). Any DDL
column that was populated from a capture now reads `NULL` (standard SQL for an
un-inserted column). To fail loudly instead of silently NULLing, a config
declaring a `{name}` placeholder whose name is **also** a declared DDL column
is now a **load-time error**. This affects the Rust core and both bindings
(the behavior lives in the shared core). The public Rust `matcher` API also
loses its capture surface.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` table whose glob placeholder names a declared column | `ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"`<br>`glob = "_comments/{thread_id}/*.txt"` (populated `thread_id` from the path) | `ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"`<br>`glob = "_comments/*/*.txt"`<br>`on-file = "python3 -c \"import sys,json,os; p=sys.argv[1]; print(json.dumps([{'thread_id': p.split(os.sep)[-2]}]))\" {path}"` (hook splits the path itself) |
| Placeholder used only for readability, no matching column | `glob = "_comments/{thread_id}/*.txt"` with a DDL that has no `thread_id` column | Unchanged — keeps working silently; `{thread_id}` matches like `*`. |
| `dirsql::matcher::MatchResult` | had a `pub captures: HashMap<String, String>` field | field removed; `MatchResult` carries only `table_name` |
| `dirsql::matcher::parse_captures` | `pub fn parse_captures(&str) -> (String, Vec<String>, Option<Regex>)` | removed; use `matcher::placeholder_names(&str) -> Vec<String>` for the placeholder names |
| `dirsql::matcher::TableMatcher::captures_for` | `pub fn captures_for(&self, &Path, &str) -> HashMap<String, String>` | removed (captures no longer exist) |

#### Deprecations removed

_None._ (Captures were never deprecation-gated; they are removed outright.)

#### Behavior changes without code changes

- **Capture columns**: a DDL column that was filled from a `{name}` glob
  placeholder is no longer populated. If the placeholder name matches the
  column name, construction now returns
  `DirSqlError::CaptureColumnCollision { placeholder, column }` instead of
  building a table whose column silently reads its captured value.
- **Matching**: unchanged. `{name}` compiles to the same `GlobSet` as `*`, so
  the set of matched files is identical before and after.

#### Verification

```sh
# A colliding config now errors at load rather than populating the column:
printf '[[table]]\nddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"\nglob = "_comments/{thread_id}/*.txt"\n' > .dirsql.toml
dirsql query "SELECT * FROM comments" -c .dirsql.toml
# expected: non-zero exit; stderr names `thread_id` and explains the collision.

# Splitting the value out of the path in the hook restores the column:
printf '[[table]]\nddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"\nglob = "_comments/*/*.txt"\non-file = "python3 -c \\"import sys,json,os; p=sys.argv[1]; print(json.dumps([{'\''thread_id'\'': p.split(os.sep)[-2]}]))\\" {path}"\n' > .dirsql.toml
dirsql query "SELECT thread_id FROM comments" -c .dirsql.toml
# expected: rows carrying the directory segment as `thread_id`.
```
