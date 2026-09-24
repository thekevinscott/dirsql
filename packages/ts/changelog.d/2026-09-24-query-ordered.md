**Added**

- `db.queryOrdered(sql)` resolves to `{ columns, rows }`: the rows `query`
  returns, plus the column names in the order the SELECT list names them. A
  row object cannot carry that order, since integer-like keys always enumerate
  first. The `QueryResult` type is exported from the package root. (#1174)
