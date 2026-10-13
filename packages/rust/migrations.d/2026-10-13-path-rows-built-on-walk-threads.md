### Faster recursive globs

#### Summary

A path table builds each row on the walk thread that found the file, and each row is smaller. Query results and their order are unchanged; nothing breaks.

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
