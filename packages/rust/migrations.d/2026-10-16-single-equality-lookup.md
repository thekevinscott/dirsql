### Faster single equality lookups

#### Summary

A path-table answers its first equality on `path`, `basename` or `dir` by comparing every row, and builds a lookup index only when the same column is probed again, as a join does. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT path FROM './**/*.md' WHERE path = 'README.md'"
```

Prints the same row as before, sooner.
