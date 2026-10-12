### Faster walks over a few large directories

#### Summary

The tree walk starts sharing directories across cores sooner and no longer spawns a thread set per large directory. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT count(*) FROM './*/*.md'"
```

Prints the same count as before, sooner.
