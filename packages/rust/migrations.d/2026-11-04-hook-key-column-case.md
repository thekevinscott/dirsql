### Hook keys match declared columns case-insensitively

#### Summary

A key from an `on-file` command or an SDK `Table` closure now fills the declared column whose name it matches ignoring ASCII case, in relaxed and `strict` tables. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A key differing from a declared column only by ASCII case stores its value where it was `NULL` before; under `strict = true` such a row is accepted where it was rejected as an extra and a missing column.

#### Verification

With `t.ndjson` holding `{"title":"hello"}` and a `[[table]]` declaring `ddl = "CREATE TABLE posts (TITLE TEXT)"`, `glob = "t.ndjson"`, `on-file = "cat"`, running `dirsql query "SELECT title FROM posts" -c c.toml --format json` prints `[{"TITLE":"hello"}]`.
