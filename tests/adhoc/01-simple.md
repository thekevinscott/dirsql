<data name="fixture">tests/fixtures/simple</data>

```python
import json
import subprocess

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

<data language="json" name="ms">{"max": 500, "mean": 50, "times_baseline": 20}</data>

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

<data name="baseline">

```bash
find . -maxdepth 1 -type f
```

</data>

## CLI

```python
@pytest.mark.limits(ms_max=ms, mb_max=mb, baseline=baseline, cwd=fixture)
@pytest.mark.parametrize(("query", "expected"), cases_query)
def test_it_runs_a_query(query, expected):
  result = subprocess.run([*cli, "query", query], cwd=fixture, capture_output=True, text=True, check=True)
  assert json.loads(result.stdout) == expected
```

## SDK

### Typescript

```typescript
test.for(cases_query as [string, Record<string, unknown>[]][])('It runs a query (%#): `%s`', { meta: { ms, mb, baseline, cwd: fixture } }, async ([query, expected]) => {
  const db = new DirSQL({ root: fixture });
  await db.ready;
  expect(await db.query(query)).toEqual(expected);
});
```

### Python

```python
@pytest.mark.limits(ms_max=ms, mb_max=mb, baseline=baseline, cwd=fixture)
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
