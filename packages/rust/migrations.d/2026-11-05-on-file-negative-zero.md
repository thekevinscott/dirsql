### A bare -0 in on-file output is INTEGER 0

#### Summary

A bare `-0` in on-file hook output (from a `[[table]]` `on-file` or `--on-file`) is stored as `INTEGER` 0 instead of `REAL` -0.0. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`typeof()` of a bare `-0` value reads `integer` where it read `real`, and the JSON output prints `0` where it printed `-0.0`. An inferred column whose values are integers plus a `-0` is now `INTEGER` instead of `TEXT`.

#### Verification

In an empty directory, write `{"e":-0}` to `z.ndjson` and run `dirsql query "SELECT e, typeof(e) t FROM './z.ndjson'" --on-file cat --format json`; it prints `[{"e":0,"t":"integer"}]`.
