### `queryOrdered` reports the projection order

**Summary**

New `db.queryOrdered(sql)` resolves to `{ columns, rows }`, where `columns`
lists the result's column names in the order the SELECT list names them.
Additive: `query` is unchanged.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```ts
const { columns } = await db.queryOrdered('SELECT name, 1 AS "2" FROM users');
// columns: ["name", "2"] -- a row's keys enumerate "2" first
```
