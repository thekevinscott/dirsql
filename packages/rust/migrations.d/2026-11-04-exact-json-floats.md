### Exact JSON float parsing

#### Summary

A JSON float an `on-file` hook or worker function prints is stored as the correctly rounded REAL, where some were one ULP off before. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A REAL column fed from JSON can hold a value one ULP different from before the upgrade: the correct one.

#### Verification

In an empty directory, write `{"v":123456789.12345679}` to `a.ndjson` and run `dirsql query "SELECT v = 123456789.12345679 AS eq FROM './a.ndjson'" --on-file cat --format json`; the output is `[{"eq":1}]`.
