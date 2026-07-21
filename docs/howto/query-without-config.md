# Query files without a config

You have a directory and a question about the files in it. You want an answer
now — not a `.dirsql.toml`, not a schema, not a setup step. Point `dirsql` at
the directory and write a path where a table name goes.

## 1. Run a query against the directory

No config, no install ceremony — `uvx` (or `npx`) fetches and runs `dirsql`,
rooted at the directory you run it from:

```bash
uvx dirsql query "SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5"
```

```json
[
  {"basename":"video.mp4","size":84213770},
  {"basename":"archive.zip","size":9123400},
  {"basename":"notes.md","size":40213},
  {"basename":"todo.md","size":1200},
  {"basename":"README.md","size":840}
]
```

`'./'` stands in for a table you never declared. `dirsql` scans the directory
live and hands SQLite one row per file — this is a
[path-table](../reference/path-tables.md). Because there is no `-c`, there are
**no named tables** at all; the path *is* the query
([configless mode](../reference/cli.md#configless-mode)).

## 2. Ask about content, not just names

Every file exposes the seven [stat columns](../reference/columns.md) (`path`,
`basename`, `dir`, `ext`, `size`, `mtime`, `ctime`) plus a hidden `content`
column. `content` is read only when you name it, so filtering on it is cheap
until you actually ask:

```bash
uvx dirsql query "SELECT path FROM './docs/**/*.md' WHERE content LIKE '%deprecated%'"
```

The `./` prefix is required. A bare glob is rejected with a hint rather than
silently guessed:

```
uvx dirsql query "SELECT * FROM '**/*.md'"
-- no such table: **/*.md; did you mean './**/*.md'?
```

## 3. Graduate to a named table when it pays off

A path-table re-scans the filesystem on every query — perfect for a one-off
question over a few hundred files, wasteful for a large tree you query
repeatedly. When that day comes, [declare a table](./define-tables.md): it is
indexed once, kept fresh by the watcher, and can persist across restarts. The
zero-config query is the floor; a named table is the escalation path — nothing
you write here has to be thrown away to get there.

## Notes

- The same query works from the SDK: construct `DirSQL` with neither a
  `config` nor `tables` and call `query("SELECT * FROM './'")`
  ([SDK reference](../reference/sdk.md)).
- Paths outside the root resolve too — `'/var/log/*.log'`, `'../notes'`,
  `'~/notes/*.md'` — reporting absolute paths
  ([path-tables reference](../reference/path-tables.md#paths-outside-the-index-root)).
- `node_modules/` and `.git/` are skipped by default so a bare `'./'` does not
  drown in machinery ([skip rules](../reference/path-tables.md#skip-rules)).
