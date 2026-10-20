### `on-file` declaration order

#### Summary

Config tables' `on-file` commands run one table at a time, in declaration order (argv order across repeated `-c`). Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

Two tables' commands no longer run at the same time; a table's command starts only after the table declared before it has finished.

#### Verification

```bash
dirsql query "SELECT 1" -c .dirsql.toml
```

Each table's command starts after the previous table's has exited.
