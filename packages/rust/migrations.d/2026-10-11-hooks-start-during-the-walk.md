### Per-table commands start during the walk

#### Summary

A per-table `on_file` command is spawned as soon as the startup walk matches its first file, rather than after the walk ends. Rows and their content are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT status, count(*) FROM notes GROUP BY status"
```

Prints the same rows as before, sooner on a large tree.
