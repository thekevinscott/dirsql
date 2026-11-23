<data name="fixture">tests/fixtures/simple</data>

```python
import pytest
```

```typescript
import { test, expect } from 'vitest';
```

```rust
use thing::run;
```

# It runs a query

<data language="json" name="ms">{"max": 500, "ok": true}</data>

<data name="cases">

```python
[
  ["SELECT 1", [{"n": 1}]],
]
```

</data>

## CLI

<data name="baseline">

```bash
find . -type f
```

</data>

```bash
dirsql query
```

## SDK

### Python

```python
@pytest.mark.parametrize(("query", "expected"), cases)
def test_it_runs_a_query(query, expected):
  assert run(fixture, query) == expected
```

### Typescript

```typescript
test.for(cases)('It runs a query', ([query, expected]) => {
  expect(run(fixture, query)).toEqual(expected);
});
```

### Rust

```rust
#[test]
fn it_runs_a_query() {
  assert!(cases.is_array());
}
```
