### `DirSQL` with no config serves the baked-in `files` table (#603)

#### Summary

A `DirSQL` constructed with neither a `config` nor programmatic `tables` now
serves the baked-in default `files` table (one row per file over the root)
instead of an empty index — parity with the CLI's no-`-c` default. The change
lives in the shared Rust core builder; the TypeScript SDK signature and the
explicit `config` option are unchanged (TypeScript never had a root-joining
shortcut).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Want an empty index (no tables) | `new DirSQL({ root })` (was table-free) | Pass at least one table; `new DirSQL({ root })` with no `config`/`tables` now serves the default `files` table |
| Read a config on disk | `new DirSQL({ root, config })` / `new DirSQL(configPath)` | unchanged — pass the path via `config` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `new DirSQL({ root })` with no `config` and no `tables` now exposes the
  baked-in `files` table; `await db.query("SELECT * FROM files")` resolves
  instead of rejecting with "no such table".

#### Verification

```ts
import { DirSQL } from "dirsql";
const db = new DirSQL({ root: "." });   // no config, no tables
await db.ready;
await db.query("SELECT basename FROM files LIMIT 1");   // ok, not "no such table: files"
```
