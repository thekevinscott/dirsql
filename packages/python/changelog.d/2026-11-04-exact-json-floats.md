**Fixed** JSON numbers from `on-file` hooks and worker functions now parse to the exact double: `123456789.12345679` was stored one ULP off as `123456789.1234568`.
