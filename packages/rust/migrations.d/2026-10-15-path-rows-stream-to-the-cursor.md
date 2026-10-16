### Path rows stream to the query

#### Summary

A path table hands rows to the query in chunks as the walk finds them, instead of after the walk ends, and passes path text to SQLite without copying it. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT path FROM './**/*.md'"
```

Prints the same rows as before, sooner.
