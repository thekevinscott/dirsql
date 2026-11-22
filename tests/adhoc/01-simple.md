<data name="fixture">tests/fixtures/simple</data>

```python
import pytest
from dirsql import DirSQL
```

```typescript
import { test, expect } from 'vitest';
import { DirSQL } from 'dirsql';
```

```rust
use dirsql::{DirSQL, Value};
```

# It runs a query

<data language="json" name="ms">{"max": 500, "mean": 50}</data>

<data language="json" name="mb">{"max": 50, "mean": 1}</data>

<data name="cases_query">

```python
[
  ["SELECT basename FROM './' ORDER BY basename", [
    {"basename": "a.txt"},
    {"basename": "b.txt"},
    {"basename": "c.txt"},
  ]],
]
```

</data>

## CLI

<data name="baseline">

```bash
find . -maxdepth 1 -type f
```

</data>

### Python

```bash
uvx dirsql query
```

### Typescript

```bash
npx dirsql query
```

### Rust

```bash
cargo install dirsql --features cli
dirsql query
```

## SDK

### Typescript

```typescript
test.for(cases_query as [string, Record<string, unknown>[]][])('It runs a query (%#): `%s`', { meta: { ms, mb } }, async ([query, expected]) => {
  const db = new DirSQL({ root: fixture });
  await db.ready();
  expect(await db.query(query)).toEqual(expected);
});
```

### Python

```python
@pytest.mark.limits(ms_max=ms, mb_max=mb)
@pytest.mark.asyncio
@pytest.mark.parametrize(("query", "expected"), cases_query)
async def test_it_runs_a_query(query, expected):
  db = DirSQL(fixture)
  await db.ready()
  assert await db.query(query) == expected
```

### Rust

```rust
#[test]
fn it_runs_a_query() {
  for case in cases_query.as_array().unwrap() {
    let query = case[0].as_str().unwrap();
    let expected = case[1].as_array().unwrap();
    let db = DirSQL::builder().root(fixture).build().unwrap();
    let rows = db.query(query).unwrap();
    assert_eq!(rows.len(), expected.len());
    for (row, want) in rows.iter().zip(expected) {
      for (column, value) in want.as_object().unwrap() {
        assert_eq!(row[column], Value::Text(value.as_str().unwrap().to_string()));
      }
    }
  }
}
```
