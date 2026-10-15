### Faster config-table startup

#### Summary

A config table's startup scan does less per-file work on one thread: the walk threads build each file's relative path, and an `on-file` command's rows are inserted while the scanned files are grouped. Query results are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT count(*) FROM notes" -c .dirsql.toml
```

Prints the same count as before, sooner.
