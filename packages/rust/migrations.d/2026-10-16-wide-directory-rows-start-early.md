### Rows of a wide directory start sooner

#### Summary

A wide directory is judged in parts, and each part's rows go out as soon as it and the parts before it are done. Query results and their order are unchanged; nothing breaks.

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

This prints the same rows as before, sooner.
