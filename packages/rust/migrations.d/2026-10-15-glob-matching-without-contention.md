### Faster globs over wide directories

#### Summary

Each walk thread matches names through its own copy of a glob, and a wide directory splits its matching only across the cores the walk leaves idle. Query results and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT path FROM './*/*.md'"
```

Prints the same rows as before, sooner.
