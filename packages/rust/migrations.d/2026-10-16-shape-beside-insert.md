### Shape beside insert

#### Summary

Startup shapes a per-table hook's rows on a separate thread from the one inserting them. No public API changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT status, count(*) FROM notes GROUP BY status" -c .dirsql.toml
```

Prints the same rows as before, sooner.
