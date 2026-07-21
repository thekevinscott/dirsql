### Glob captures removed; colliding placeholder + column is a load-time error (#655)

#### Summary

Glob `{name}` placeholders no longer populate columns. `{name}` still matches
like `*`, so the set of matched files is unchanged, but a placeholder whose
name is also a declared DDL column now rejects at load — the `ready` promise
rejects (or the first awaited query that triggers the build). A placeholder
with no matching column keeps working silently. To keep a column that used to
come from a capture, split the value out of the path in your `onFile` hook.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Table / config whose glob placeholder names a declared column | `{ ddl: "CREATE TABLE comments (thread_id TEXT)", glob: "_comments/{thread_id}/*.txt", onFile: ... }` (relied on the capture) | `{ ddl: "CREATE TABLE comments (thread_id TEXT)", glob: "_comments/*/*.txt", onFile: (p) => [{ thread_id: p.split("/").at(-2) }] }` (hook splits the path) |
| Placeholder with no matching column | `glob: "_comments/{thread_id}/*.txt"` with no `thread_id` column | Unchanged — still matches like `*`. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A column previously populated from a `{name}` capture now reads `null`; if
  the placeholder name matches the column name, `ready` rejects instead of
  silently nulling the column. The error message names the placeholder and the
  fix.
- Matching is unchanged: `{name}` behaves like `*`.

#### Verification

```ts
import { DirSQL } from "dirsql";

const db = new DirSQL({
  root: "/data",
  tables: [{
    ddl: "CREATE TABLE comments (thread_id TEXT, basename TEXT)",
    glob: "_comments/*/*.txt",
    onFile: (p) => [{ thread_id: p.split("/").at(-2) }],
  }],
});
await db.ready;
console.log(await db.query("SELECT thread_id FROM comments"));
// expected: rows carrying the directory segment as thread_id.
```
