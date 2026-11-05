### SQL after the first statement is rejected by query

#### Summary

`query` (CLI one-shot, `dirsql query`, REPL, `POST /query`, every SDK) now rejects SQL that holds more than one statement, with `query() runs one statement at a time; remove the SQL after the first statement` (`DbError::MultipleStatements`; HTTP `400`). Previously only the first statement ran and the rest was silently ignored. A trailing `;`, whitespace, or comments are unchanged.

#### Required changes

| Before | After |
|---|---|
| `dirsql "SELECT 1 AS a; SELECT 2 AS b"` | `dirsql "SELECT 1 AS a"` then `dirsql "SELECT 2 AS b"`, or one statement with `UNION ALL` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query string with text after its first statement now errors instead of returning the first statement's rows; in the REPL the session continues, a one-shot query exits non-zero.

#### Verification

`dirsql "SELECT 1 AS a; SELECT 2 AS b" --format json` prints `query() runs one statement at a time; ...` on stderr and exits 1; `dirsql "SELECT 1 AS a;" --format json` prints `[{"a":1}]`.
