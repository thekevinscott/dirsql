# `dirsql` (TypeScript SDK)

Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface -- the filesystem is always the source of truth. Built on the Rust core via napi-rs bindings.

[Documentation](https://thekevinscott.github.io/dirsql/?lang=typescript)

Also available as [`dirsql` on crates.io](https://crates.io/crates/dirsql) and [`dirsql` on PyPI](https://pypi.org/project/dirsql/).

## Installation

```bash
pnpm add dirsql
```

Prebuilt binaries ship for linux-x64, linux-arm64, darwin-x64, darwin-arm64, and win32-x64; npm / pnpm pick up the right one via `optionalDependencies`, so no Rust toolchain is required. The npm CLI requires **Node >= 20.11**.

## Quick start

Constructing a `DirSQL` returns immediately; scanning runs in the background and every method awaits it, so you can query right away (or `await db.ready` to surface scan errors up front). Each table is a `(ddl, glob, extract)` object: the DDL defines the SQLite schema, the glob selects files (relative to `root`), and `extract` returns the rows a matched file contributes -- always an array, return `[]` to skip a file. `dirsql` does not read file contents; if `extract` needs the file body it reads `path` itself.

```typescript
import { readFileSync } from "node:fs";
import { DirSQL, type TableDef } from "dirsql";

const tables: TableDef[] = [
  {
    ddl: "CREATE TABLE posts (title TEXT, author TEXT)",
    glob: "posts/*.json",
    extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
  },
];

const db = new DirSQL({ root: "./my-blog", tables });

const posts = await db.query("SELECT * FROM posts WHERE author = 'alice'");
console.log(posts);
```

## Multiple tables and joins

```typescript
import { readFileSync } from "node:fs";
import { DirSQL, type TableDef } from "dirsql";

const tables: TableDef[] = [
  {
    ddl: "CREATE TABLE posts (title TEXT, author_id TEXT)",
    glob: "posts/*.json",
    extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
  },
  {
    ddl: "CREATE TABLE authors (id TEXT, name TEXT)",
    glob: "authors/*.json",
    extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
  },
];

const db = new DirSQL({ root: "./my-blog", tables });

const results = await db.query(`
  SELECT posts.title, authors.name
  FROM posts JOIN authors ON posts.author_id = authors.id
`);
```

## Ignoring files

Pass `ignore` patterns to skip files during scanning and watching:

```typescript
const db = new DirSQL({
  root: "./my-blog",
  tables: [/* ... */],
  ignore: ["**/drafts/**", "**/.git/**"],
});
```

## Watching for changes

`db.watch()` returns an async iterable of row-level change events as files change on disk:

```typescript
for await (const event of db.watch()) {
  console.log(`${event.action} on ${event.table}:`, event.row);
}
```

Each event has `.action` (`'insert'` | `'update'` | `'delete'` | `'error'`), `.table`, `.row` (the new row, or the deleted row on `delete`), `.oldRow` (the previous row, on `update`), `.filePath`, and `.error` (on `error`).

## CLI

`npx dirsql` runs an HTTP server exposing the SDK over HTTP: `POST /query` for SQL and `GET /events` for a Server-Sent Events change stream. Requires **Node >= 20.11**. See the [CLI guide](https://thekevinscott.github.io/dirsql/cli/).

## License

MIT
