### Inferred path-table columns fold keys differing only in case

#### Summary

A parsed path-table (`--on-file`, or the SDK's `path_table_parser`) now infers one column for keys that differ only in ASCII case, named by the first-seen spelling. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query over parser output whose keys differ only in ASCII case returns rows where it failed before with `vtable constructor failed`.

#### Verification

With `c.ndjson` holding `{"title":"lower"}` and `{"Title":"upper"}` on two lines, running `dirsql query "SELECT * FROM './c.ndjson'" --on-file cat --format json` prints `[{"title":"lower"},{"title":"upper"}]`.
