# API Reference

## DirSQL

```python
from dirsql import DirSQL
```

### Constructor

```python
DirSQL(root: str, *, tables: list[Table], ignore: list[str] | None = None)
```

Creates an in-memory SQLite index over the given directory. The constructor scans the directory synchronously and blocks until indexing is complete.

**Parameters:**

- `root` -- Path to the directory to index.
- `tables` -- List of `Table` definitions. Each defines a SQLite table, a glob pattern, and an extract function.
- `ignore` -- Optional list of glob patterns. Files matching any ignore pattern are skipped regardless of table globs.

**Raises:** `RuntimeError` if DDL is invalid, a file cannot be read, or the database fails to initialize.

### Methods

#### `query(sql: str) -> list[dict]`

Execute a SQL query against the in-memory database. Returns a list of dicts keyed by column name. Internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) are excluded from results.

**Raises:** `RuntimeError` on invalid SQL.

---

## AsyncDirSQL

```python
from dirsql import AsyncDirSQL
```

### Constructor

```python
AsyncDirSQL(root: str, *, tables: list[Table], ignore: list[str] | None = None)
```

Async wrapper around `DirSQL`. Starts the directory scan in a background thread immediately. The constructor returns without blocking.

**Parameters:** Same as `DirSQL`.

### Methods

#### `await ready() -> None`

Wait for the initial scan to complete. Re-raises any exception from the scan. Safe to call multiple times.

#### `await query(sql: str) -> list[dict]`

Run a SQL query in a background thread. Same return format as `DirSQL.query()`.

#### `watch() -> AsyncIterator[RowEvent]`

Returns an async iterator of `RowEvent` objects. The file watcher starts automatically on first iteration. The iterator never terminates on its own.

---

## Table

```python
from dirsql import Table
```

### Constructor

```python
Table(*, ddl: str, glob: str, extract: Callable[[str, str], list[dict]])
```

Defines a mapping from files to SQLite table rows.

**Parameters:**

- `ddl` -- A `CREATE TABLE` statement. The table name is parsed from this DDL.
- `glob` -- A glob pattern matched against file paths relative to the root directory.
- `extract` -- A callable `(path, content) -> list[dict]`. Receives the relative file path and file content as strings. Returns a list of dicts mapping column names to values. Return `[]` to skip a file.

**Attributes:**

- `ddl: str` -- The DDL string (read-only).
- `glob: str` -- The glob pattern (read-only).

---

## RowEvent

```python
from dirsql import RowEvent
```

Emitted by `AsyncDirSQL.watch()`. Represents a change to a row in the database caused by a filesystem event.

**Attributes:**

- `table: str` -- The table name.
- `action: str` -- One of `"insert"`, `"update"`, `"delete"`, `"error"`.
- `row: dict | None` -- The new/current row (for insert and update) or the deleted row (for delete). `None` for errors.
- `old_row: dict | None` -- The previous row (for update only). `None` for insert, delete, and error.
- `error: str | None` -- Error message (for error events only).
- `file_path: str | None` -- The relative file path that triggered this event.

**String representation:**

```python
repr(event)  # RowEvent(table='comments', action='insert')
```
