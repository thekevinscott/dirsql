**Fixed** A query returning a text value that is not valid UTF-8 (for example
`CAST(x'ff' AS TEXT)` or `char(55296)`) now fails with
`column "<name>" holds text that is not valid UTF-8`, in every CLI format and
SDK. It used to crash `--format table` and every SDK `query`, and the crash
left a Python or TypeScript `DirSQL` instance failing every later query with
a poisoned lock; the instance now stays usable. `--format json`, which printed
such text with replacement characters, fails the same way.
