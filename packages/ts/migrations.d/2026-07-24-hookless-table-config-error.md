### Hook-less `[[table]]` is now a config error (#634)

#### Summary

A `[[table]]` with no `on-file` hook previously loaded fine and — after the
stat-fact injection layer was removed — produced rows that were all-NULL.
Loading such a config through the TypeScript SDK (`new DirSQL(configPath)`,
awaited via `db.ready`) now rejects. No API signature changes — this is a
load-time validation change surfaced from the core.

#### Required changes

| Before (loaded, all-NULL rows) | After (rejects) | Fix |
| ------------------------------ | --------------- | --- |
| `.dirsql.toml` `[[table]]` with `ddl` + `glob` but no `on-file` | `db.ready` rejects: `[[table]] '<glob>' has no on-file hook …` | Add an `on-file` hook that emits the declared columns. |
| You only wanted stat columns and wrote no hook code | `db.ready` rejects | Drop the `[[table]]` and query the path directly: `SELECT path, size, mtime FROM './'` (globbable: `FROM './**/*.md'`). |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `[[table]]` with no `on-file` hook, which used to load (and, after injection
  removal, yield all-NULL rows), now rejects at config load, with an error
  naming the offending glob and pointing at the `FROM './'` path-table
  replacement.

#### Verification

```ts
import { DirSQL } from "dirsql";

// .dirsql.toml with a hook-less table:
//   [[table]]
//   ddl  = "CREATE TABLE files (path TEXT, size INTEGER)"
//   glob = "**/*.md"

const db = new DirSQL(".dirsql.toml");
// Before this change: await db.ready resolved; SELECT * FROM files -> all-NULL rows.
// After this change:
try {
  await db.ready;
} catch (e) {
  console.log(String(e)); // -> ... has no on-file hook ... query the path directly: `FROM './'`
}

// Replacement with no hook code — query the path directly (no config):
const db2 = new DirSQL({ root: "." });
await db2.ready;
console.log(await db2.query("SELECT path, size FROM './**/*.md'"));
```
