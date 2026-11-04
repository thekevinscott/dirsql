### Queries keep reading the index as applied

#### Summary

The TypeScript binding opts out of the core's new query-time re-read. Nothing changes for TypeScript callers and nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

`await db.query("SELECT 1 AS n")` returns `[{ n: 1 }]`.
