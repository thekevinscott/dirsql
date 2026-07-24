### Stat-fact injection removed (#634)

#### Summary

dirsql previously injected filesystem-derived values into any DDL column named
`path`, `basename`, `dir`, `ext`, `size`, `mtime`, or `ctime` — filling them
even when a table's `onFile` callback returned nothing for that column. That
injection layer (in the shared Rust core) is gone. A `TableDef` now contains
**exactly what its `onFile` callback returns**; a declared column the callback
never emits is `null`/`NULL`. No TypeScript API signature changes — runtime
behavior only.

#### Required changes

| Before (injected) | After (NULL unless emitted) | Fix |
| ----------------- | --------------------------- | --- |
| DDL declares `path TEXT`, callback omits it → column held the root-relative path | Column is `null` | Emit it from the callback: `{ ddl: "CREATE TABLE t (path TEXT)", glob: "**/*", onFile: (path) => [{ path }] }`. |
| DDL declares `size INTEGER`, callback omits it → column held the file size | Column is `null` | Emit it: `onFile: (path) => [{ size: statSync(path).size }]`. |
| Want stat columns with **no** callback at all | (was injection) | Query the path directly with a path-table: `FROM './'`. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A DDL column named `path`/`basename`/`dir`/`ext`/`size`/`mtime`/`ctime` that
  the `onFile` callback does not emit is now `null`/`NULL` instead of an
  injected filesystem value.
- A **strict** table whose DDL declares such a column but whose callback does
  not emit it now rejects with a schema-validation error rather than being
  silently filled.

#### Verification

```ts
import { DirSQL } from "dirsql";

// Callback emits only "size"; DDL also declares "path".
const db = new DirSQL({
  root,
  tables: [{
    ddl: "CREATE TABLE files (path TEXT, size INTEGER)",
    glob: "**/*.txt",
    onFile: (_path) => [{ size: 42 }],
  }],
});
const rows = await db.query("SELECT path, size FROM files");
// Before: rows[0].path === "a.txt"  (injected)
// After:  rows[0].path === null      (not emitted)

// Restore by emitting it:
//   onFile: (path) => [{ path, size: 42 }]
```
