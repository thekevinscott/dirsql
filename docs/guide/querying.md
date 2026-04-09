# Querying

Once a `DirSQL` instance is created, the initial directory scan is complete and you can run SQL queries against the indexed data.

## Basic queries

```python
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

Any valid SQLite SQL works. The in-memory database supports the full SQLite dialect including subqueries, CTEs, window functions, and aggregate functions.

## Return format

`query()` returns a list of dicts. Each dict maps column names to Python values.

```python
results = db.query("SELECT title, author FROM posts")
# [
#     {"title": "Hello World", "author": "alice"},
#     {"title": "Second Post", "author": "bob"},
# ]
```

SQLite types map back to Python types:

| SQLite type | Python type |
|-------------|-------------|
| TEXT        | `str`       |
| INTEGER     | `int`       |
| REAL        | `float`     |
| BLOB        | `bytes`     |
| NULL        | `None`      |

## Internal columns

dirsql adds internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) to each table for file-change diffing. These columns are automatically excluded from `SELECT *` results. You do not need to account for them.

## Error handling

Invalid SQL raises a Python exception:

```python
try:
    db.query("NOT VALID SQL")
except Exception as e:
    print(f"Query error: {e}")
```

## Empty results

Queries that match no rows return an empty list:

```python
results = db.query("SELECT * FROM posts WHERE author = 'nobody'")
assert results == []
```
