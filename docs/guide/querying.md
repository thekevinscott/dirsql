# Querying

Once a `DirSQL` instance is created, the initial directory scan is complete and you can run SQL queries against the indexed data.

## Basic queries

::: code-group

```python [Python]
# All rows from a table
results = db.query("SELECT * FROM comments")

# Filter with WHERE
results = db.query("SELECT * FROM comments WHERE author = 'alice'")

# Aggregations
results = db.query("SELECT author, COUNT(*) as n FROM comments GROUP BY author")

# JOINs across tables
results = db.query("""
    SELECT posts.title, authors.name
    FROM posts
    JOIN authors ON posts.author_id = authors.id
""")
```

```rust [Rust]
// All rows from a table
let results = db.query("SELECT * FROM comments")?;

// Filter with WHERE
let results = db.query("SELECT * FROM comments WHERE author = 'alice'")?;

// Aggregations
let results = db.query("SELECT author, COUNT(*) as n FROM comments GROUP BY author")?;

// JOINs across tables
let results = db.query(
    "SELECT posts.title, authors.name \
     FROM posts JOIN authors ON posts.author_id = authors.id"
)?;
```

```typescript [TypeScript]
// All rows from a table
const results = await db.query('SELECT * FROM comments');

// Filter with WHERE
const filtered = await db.query("SELECT * FROM comments WHERE author = 'alice'");

// Aggregations
const counts = await db.query('SELECT author, COUNT(*) as n FROM comments GROUP BY author');

// JOINs across tables
const joined = await db.query(`
  SELECT posts.title, authors.name
  FROM posts
  JOIN authors ON posts.author_id = authors.id
`);
```

:::

Any valid SQLite **SELECT** works. The in-memory database supports the full SQLite dialect including subqueries, CTEs, window functions, and aggregate functions. See [Read-only queries](#read-only-queries) below for why write statements (`INSERT`, `UPDATE`, `DELETE`, `DROP`, etc.) are rejected.

## Return format

`query()` returns a list of dicts (Python), a `Vec<HashMap>` (Rust), or an array of objects (TypeScript). Each entry maps column names to values.

::: code-group

```python [Python]
results = db.query("SELECT title, author FROM posts")
# [
#     {"title": "Hello World", "author": "alice"},
#     {"title": "Second Post", "author": "bob"},
# ]
```

```rust [Rust]
let results = db.query("SELECT title, author FROM posts")?;
// Vec<HashMap<String, Value>>
// [{"title": "Hello World", "author": "alice"}, ...]
```

```typescript [TypeScript]
const results = await db.query('SELECT title, author FROM posts');
// [
//   { title: 'Hello World', author: 'alice' },
//   { title: 'Second Post', author: 'bob' },
// ]
```

:::

SQLite types map back to Python types:

| SQLite type | Python type |
|-------------|-------------|
| TEXT        | `str`       |
| INTEGER     | `int`       |
| REAL        | `float`     |
| BLOB        | `bytes`     |
| NULL        | `None`      |

## Internal columns

`dirsql` adds internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) to each table for file-change diffing. These columns are automatically excluded from `SELECT *` results. You do not need to account for them.

## Read-only queries

`query()` accepts only read-only statements. The first non-comment keyword must be `SELECT` or `WITH` (for a CTE that feeds a `SELECT`); any other leading keyword — `INSERT`, `UPDATE`, `DELETE`, `DROP`, `CREATE`, `ALTER`, `ATTACH`, `PRAGMA`, `VACUUM`, `REPLACE`, etc. — is rejected before it reaches SQLite.

This keeps the in-memory index consistent with the on-disk files that back it. Mutations only happen through the watcher/indexer pipeline: to change data, edit the underlying file and let the watcher re-extract rows.

::: code-group

```python [Python]
# Raises a RuntimeError; the index is unchanged.
db.query("DELETE FROM posts")
```

```rust [Rust]
// Returns DirSqlError::WriteForbidden; the index is unchanged.
let err = db.query("DELETE FROM posts").unwrap_err();
assert!(matches!(err, dirsql::DirSqlError::WriteForbidden { .. }));
```

```typescript [TypeScript]
// Throws an Error whose message explains writes are not accepted.
expect(() => db.query('DELETE FROM posts')).toThrow(/read-only/i);
```

:::

## Error handling

Invalid SQL raises an exception:

::: code-group

```python [Python]
try:
    db.query("NOT VALID SQL")
except Exception as e:
    print(f"Query error: {e}")
```

```rust [Rust]
match db.query("NOT VALID SQL") {
    Ok(results) => println!("{:?}", results),
    Err(e) => eprintln!("Query error: {}", e),
}
```

```typescript [TypeScript]
try {
  await db.query('NOT VALID SQL');
} catch (e) {
  console.error(`Query error: ${e}`);
}
```

:::

## Empty results

Queries that match no rows return an empty collection:

::: code-group

```python [Python]
results = db.query("SELECT * FROM posts WHERE author = 'nobody'")
assert results == []
```

```rust [Rust]
let results = db.query("SELECT * FROM posts WHERE author = 'nobody'")?;
assert!(results.is_empty());
```

```typescript [TypeScript]
const results = await db.query("SELECT * FROM posts WHERE author = 'nobody'");
console.assert(results.length === 0);
```

:::
