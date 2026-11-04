### BEGIN, SAVEPOINT and PRAGMA setters are rejected by query

#### Summary

`query` now rejects transaction control (`BEGIN`, `COMMIT`, `END`, `ROLLBACK`), savepoints (`SAVEPOINT`, `RELEASE`, `ROLLBACK TO`) and any `PRAGMA` carrying a value, other than the pragmas whose argument names what to read (`table_info`, `table_xinfo`, `table_list`, `index_info`, `index_xinfo`, `index_list`, `foreign_key_list`, `foreign_key_check`, `integrity_check`, `quick_check`). They fail with a `not authorized` error (`DbError::Unauthorized`; HTTP `400`). Value-less pragmas and every `SELECT` are unchanged.

#### Required changes

| Before | After |
|---|---|
| `PRAGMA case_sensitive_like=1` then `... LIKE 'X'` | `... WHERE x GLOB 'X'` (case-sensitive match in one statement) |
| `BEGIN` ... `COMMIT` around queries | Drop them; each query reads the current index |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A script that sent `BEGIN` or a `PRAGMA` setter through `query` and ignored the empty result now gets an error; in the REPL the session continues, a one-shot `dirsql "BEGIN"` exits non-zero.

#### Verification

`printf "PRAGMA case_sensitive_like=1\nSELECT 'a' LIKE 'A' AS m\n" | dirsql --format json` prints `not authorized: ...` on stderr for the first line and `[{"m":1}]` for the second.
