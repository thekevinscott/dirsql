### Faster sorts over large tables

#### Summary

SQLite may now sort on up to eight background threads. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT level, count(*) FROM events GROUP BY level"
```

Prints the same rows as before, sooner on a large table.
