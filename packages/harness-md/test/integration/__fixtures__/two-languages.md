```python
import pytest
```

```typescript
import { test, expect } from 'vitest';
```

# It adds

Prose between blocks is left out.

```python
def test_it_adds():
  assert 1 + 1 == 2
```

```typescript
test('It adds', () => {
  expect(1 + 1).toBe(2);
});
```
