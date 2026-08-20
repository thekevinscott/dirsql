### `TableDef` requires an explicit `name`; `parseTableName` removed (#962)

#### Summary

A table's SQL name was derived by parsing the `CREATE TABLE` head of `ddl`, and
that parser was exposed as the top-level `parseTableName(ddl)` export. The name
is now **declared**: `TableDef` (and the `Table` class) requires a `name`, and a
`[[table]]` entry in a `.dirsql.toml` requires a `name` key. Whether the `ddl`
creates a table by that name is checked against SQLite's catalog at load time,
so the parser — and its export — are gone.

#### Required changes

| Before | After | Fix |
| ------ | ----- | --- |
| `{ ddl, glob, onFile }` / `new Table({ ddl, glob, onFile })` | `Property 'name' is missing in type … but required in type 'TableDef'` | Add `name: "<table>"`, matching the table the `ddl` creates. |
| `[[table]]` with `ddl` + `glob` + `on-file` | `await db.ready` rejects: `Missing required field 'name' in [[table]] entry` | Add `name = "<table>"` to the entry. |
| `import { parseTableName } from "dirsql"` | Export no longer exists | Use the name you declared on the table. |

#### Deprecations removed

- `parseTableName(ddl)` — removed outright, along with the core DDL tokenizer
  it wrapped.

#### Behavior changes without code changes

- A `[[table]]` whose `ddl` creates a table under a different name than its
  `name` now rejects at `ready` with an error naming the entry
  (`table 'messages': …`), before any file is ingested.
- Quoted (`CREATE TABLE "messages"`), schema-qualified and `IF NOT EXISTS` DDL
  keep working unchanged: SQLite records the bare identifier, so a plain
  `name: "messages"` matches.

#### Verification

```typescript
import { readFileSync } from "node:fs";
import { DirSQL } from "dirsql";

const db = new DirSQL({
  root: "./data-root",
  tables: [
    {
      name: "records",
      ddl: "CREATE TABLE records (id TEXT)",
      glob: "data/*.json",
      onFile: (path) => JSON.parse(readFileSync(path, "utf8")),
    },
  ],
});
await db.query("SELECT id FROM records");
// -> the file's rows. Omitting `name` fails to typecheck; a `name` the ddl
//    never creates rejects `db.ready`.
```
