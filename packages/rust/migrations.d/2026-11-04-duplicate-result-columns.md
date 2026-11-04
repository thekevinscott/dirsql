### Duplicate result columns retain every value

#### Summary

Queries that repeat a result column name now keep every value across the Rust API and CLI, as well as bindings that use the shared core. Existing queries with unique names are unchanged. Nothing breaks for queries with unique names.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

Repeated column names receive distinct `:N` suffixes in projection order. A suffix skips any name already present in the result.

#### Verification

Run `dirsql 'SELECT 1 AS a, 2 AS a' --format json` and expect `[{"a":1,"a:1":2}]`.
