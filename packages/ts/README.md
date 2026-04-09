# dirsql (TypeScript SDK)

Ephemeral SQL index over a local directory. Pure TypeScript implementation using better-sqlite3.

## Installation

```bash
npm install dirsql
```

## Quick Start

```typescript
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL('/path/to/data', {
  tables: [
    new Table({
      ddl: 'CREATE TABLE posts (title TEXT, author TEXT)',
      glob: 'posts/*.json',
      extract: (path, content) => [JSON.parse(content)],
    }),
  ],
});

await db.ready;

const posts = db.query('SELECT * FROM posts');
console.log(posts);
```

## Async API

```typescript
// Watch for file changes
for await (const event of db.watch()) {
  console.log(event.action, event.table, event.row);
}
```

## API

### `new DirSQL(root, options)`

- `root` -- directory path to index
- `options.tables` -- array of `Table` definitions
- `options.ignore` -- optional array of glob patterns to skip

### `await db.ready`

Awaitable property. Resolves when initial scan is complete.

### `db.query(sql)`

Execute a SQL query. Returns array of objects.

### `db.watch()`

Returns an `AsyncIterable<RowEvent>` that yields events as files change.

### `new Table({ ddl, glob, extract })`

- `ddl` -- CREATE TABLE SQL statement
- `glob` -- file pattern to match
- `extract` -- `(path: string, content: string) => Record<string, unknown>[]`

### `RowEvent`

- `action` -- `'insert' | 'update' | 'delete' | 'error'`
- `table` -- table name
- `row` -- row data (for insert/update/delete)
- `oldRow` -- previous row data (for update)
- `error` -- error message (for error events)
- `filePath` -- source file path (for error events)

## License

MIT
