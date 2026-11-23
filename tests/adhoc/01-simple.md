<data name="fixture">tests/fixtures/simple</data>

# It runs a query

<data language="json" name="ms">{"max": 500, "mean": 50, "times_baseline": 20}</data>

<data language="json" name="mb">{"max": 50, "mean": 1, "times_baseline": 40}</data>

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

### Typescript

```typescript
import { execFileSync } from 'node:child_process';
import { test, expect } from 'vitest';

test.for(cases_query as [string, Record<string, unknown>[]][])('It runs a query (%#): `%s`', { meta: { ms, mb, baseline, cwd: fixture, cli } }, ([query, expected]) => {
  const stdout = execFileSync(cli[0], [...cli.slice(1), 'query', query], { cwd: fixture, encoding: 'utf8' });
  expect(JSON.parse(stdout)).toEqual(expected);
});
```

### Rust

```rust
#[test]
fn it_runs_a_query() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(fixture);
  let launcher: Vec<String> = cli.as_array().unwrap().iter().map(|part| part.as_str().unwrap().to_string()).collect();
  dirsql_md_tests::measured(&ms, &mb, baseline, &root, Some(&launcher), |launcher| {
    for case in cases_query.as_array().unwrap() {
      let output = std::process::Command::new(&launcher[0])
        .args(&launcher[1..])
        .args(["query", case[0].as_str().unwrap()])
        .current_dir(&root)
        .output()
        .unwrap();
      let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
      assert_eq!(rows, case[1]);
    }
  });
}
```

### Python

```python
import json
import subprocess

import pytest

@pytest.mark.limits(ms=ms, mb=mb, baseline=baseline, cwd=fixture, cli=cli)
@pytest.mark.parametrize(("query", "expected"), cases_query)
def test_it_runs_a_query(query, expected):
  result = subprocess.run([*cli, "query", query], cwd=fixture, capture_output=True, text=True, check=True)
  assert json.loads(result.stdout) == expected
```

## SDK

### Typescript

```typescript
import { test, expect } from 'vitest';
import { DirSQL } from 'dirsql';

test.for(cases_query as [string, Record<string, unknown>[]][])('It runs a query (%#): `%s`', { meta: { ms, mb, baseline, cwd: fixture } }, async ([query, expected]) => {
  const db = new DirSQL({ root: fixture });
  await db.ready;
  expect(await db.query(query)).toEqual(expected);
});
```

### Python

```python
import pytest
from dirsql import DirSQL

@pytest.mark.limits(ms=ms, mb=mb, baseline=baseline, cwd=fixture)
@pytest.mark.asyncio
@pytest.mark.parametrize(("query", "expected"), cases_query)
async def test_it_runs_a_query(query, expected):
  db = DirSQL(fixture)
  await db.ready()
  assert await db.query(query) == expected
```

### Rust

```rust
use dirsql::{DirSQL, Value};
use serde_json::json;

fn as_json(value: &Value) -> serde_json::Value {
  match value {
    Value::Null => json!(null),
    Value::Integer(integer) => json!(integer),
    Value::Real(real) => json!(real),
    Value::Text(text) => json!(text),
    Value::Blob(bytes) => json!(bytes),
  }
}

#[test]
fn it_runs_a_query() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(fixture);
  dirsql_md_tests::measured(&ms, &mb, baseline, &root, None, |_| {
    for case in cases_query.as_array().unwrap() {
      let db = DirSQL::builder().root(&root).build().unwrap();
      let rows: Vec<serde_json::Value> = db
        .query(case[0].as_str().unwrap())
        .unwrap()
        .iter()
        .map(|row| row.iter().map(|(column, value)| (column.clone(), as_json(value))).collect())
        .collect();
      assert_eq!(json!(rows), case[1]);
    }
  });
}
```
