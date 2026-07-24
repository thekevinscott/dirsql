### Stat-fact injection removed (#634)

#### Summary

dirsql previously injected filesystem-derived values into any DDL column named
`path`, `basename`, `dir`, `ext`, `size`, `mtime`, or `ctime` — filling them
even when the `on-file` hook emitted nothing for that column. That injection
layer is gone. A named table now contains **exactly what its hook emits**; a
declared column the hook never emits is `NULL` (standard SQL). No API signature
changes — this is a runtime behavior change.

#### Required changes

| Before (injected) | After (NULL unless emitted) | Fix |
| ----------------- | --------------------------- | --- |
| DDL declares `path TEXT`, hook emits neither `path` nor a value for it → column held the root-relative path | Column is `NULL` | Emit it from the hook. Config `on-file` shell hook (`$1` = abs path, `$2` = index root): `rel=${1#"$2"/}; jq -n --arg path "$rel" '[{path:$path}]'`. Rust SDK closure: `\|path\| vec![HashMap::from([("path".into(), Value::Text(path.to_string()))])]`. |
| DDL declares `size INTEGER`, hook emits neither → column held the file size | Column is `NULL` | Emit it from the hook: `jq -n --argjson size "$(stat -c%s "$1")" '[{size:$size}]'`, or in Rust read `std::fs::metadata(path)?.len()`. |
| Want stat columns with **no** hook code at all | (was injection) | Query the path directly with a path-table: `FROM './'`. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A DDL column named `path`/`basename`/`dir`/`ext`/`size`/`mtime`/`ctime` that
  the `on-file` hook does not emit is now `NULL` instead of an injected
  filesystem value. Configs and SDK tables that relied on injection must emit
  the column themselves (see the table above).
- A **strict** table whose DDL declares such a column but whose hook does not
  emit it now fails schema validation (`missing columns for table …`) rather
  than being silently filled.

#### Verification

Given `a.txt` in the index root and a non-strict table whose DDL is
`CREATE TABLE files (path TEXT, size INTEGER)` and whose hook emits only
`size`:

```bash
# Config flags follow the subcommand: `dirsql query <sql> -c <cfg>`.

# Before this change (hook emits only size):
dirsql query "SELECT path, size FROM files" -c ./.dirsql.toml
# -> [{"path":"a.txt","size":42}]   (path injected)

# After this change (same config):
dirsql query "SELECT path, size FROM files" -c ./.dirsql.toml
# -> [{"path":null,"size":42}]      (path NULL — not emitted)

# Restore it by emitting path from the hook:
#   on-file = 'rel=${1#"$2"/}; jq -n --arg path "$rel" '\''[{path:$path}]'\'''
dirsql query "SELECT path, size FROM files" -c ./.dirsql.toml
# -> [{"path":"a.txt", ...}]        (path populated again)
```
