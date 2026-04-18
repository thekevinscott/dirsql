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

Any valid SQLite SQL works. The in-memory database supports the full SQLite dialect including subqueries, CTEs, window functions, and aggregate functions.

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

`dirsql` adds internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) to each table for file-change diffing. These columns are automatically excluded from `SELECT *` results, so day-to-day queries don't need to account for them.

If you want to know which file a row came from, you can name the tracking columns explicitly in the projection:

::: code-group

```python [Python]
rows = db.query("SELECT title, _dirsql_file_path FROM posts")
# [{"title": "Hello World", "_dirsql_file_path": "posts/hello.json"}, ...]
```

```rust [Rust]
let rows = db.query("SELECT title, _dirsql_file_path FROM posts")?;
// [{"title": "Hello World", "_dirsql_file_path": "posts/hello.json"}, ...]
```

```typescript [TypeScript]
const rows = await db.query('SELECT title, _dirsql_file_path FROM posts');
// [{ title: 'Hello World', _dirsql_file_path: 'posts/hello.json' }, ...]
```

:::

Tracking columns are only returned when named explicitly — `SELECT *` continues to exclude them.

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
