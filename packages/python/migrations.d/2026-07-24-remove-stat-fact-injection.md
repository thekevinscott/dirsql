### Stat-fact injection removed (#634)

#### Summary

dirsql previously injected filesystem-derived values into any DDL column named
`path`, `basename`, `dir`, `ext`, `size`, `mtime`, or `ctime` — filling them
even when a table's `on_file` callback emitted nothing for that column. That
injection layer (in the shared Rust core) is gone. A `Table` now contains
**exactly what its `on_file` callback returns**; a declared column the callback
never emits is `None`/`NULL`. No Python API signature changes — runtime
behavior only.

#### Required changes

| Before (injected) | After (NULL unless emitted) | Fix |
| ----------------- | --------------------------- | --- |
| DDL declares `path TEXT`, callback omits it → column held the root-relative path | Column is `None` | Emit it from the callback: `Table("CREATE TABLE t (path TEXT)", "**/*", on_file=lambda path: [{"path": path}])`. |
| DDL declares `size INTEGER`, callback omits it → column held the file size | Column is `None` | Emit it: `on_file=lambda path: [{"size": os.path.getsize(path)}]`. |
| Want stat columns with **no** callback at all | (was injection) | Query the path directly with a path-table: `FROM './'`. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A DDL column named `path`/`basename`/`dir`/`ext`/`size`/`mtime`/`ctime` that
  the `on_file` callback does not emit is now `None`/`NULL` instead of an
  injected filesystem value.
- A **strict** table whose DDL declares such a column but whose callback does
  not emit it now raises a schema-validation error rather than being silently
  filled.

#### Verification

```python
from dirsql import DirSQL, Table

# Callback emits only "size"; DDL also declares "path".
db = DirSQL(root, tables=[Table(
    ddl="CREATE TABLE files (path TEXT, size INTEGER)",
    glob="**/*.txt",
    on_file=lambda _path: [{"size": 42}],
)])
rows = db.query("SELECT path, size FROM files")
# Before: rows[0]["path"] == "a.txt"  (injected)
# After:  rows[0]["path"] is None      (not emitted)

# Restore by emitting it:
#   on_file=lambda path: [{"path": path, "size": 42}]
```
