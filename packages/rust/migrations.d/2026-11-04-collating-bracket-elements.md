### Collating symbols and equivalence classes in a glob bracket

#### Summary

A bracket expression in a path-table or config glob now honors `[.name.]` and `[=c=]` elements as bash does, where they previously matched nothing. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A glob such as `./[[.a.]].md` or `./[[=a=]].md`, which returned no rows, now returns `a.md`.

#### Verification

In a directory holding `a.md` and `b.md`, `dirsql query "SELECT path FROM './[[=a=]].md'" --format json` prints `[{"path":"a.md"}]`.
