### Plain scans without stepping

#### Summary

A query that only projects stat columns of one path table reads its rows without stepping SQLite. Output is byte-identical; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql "SELECT path FROM './**/*.md'"
```

Prints the same rows as before, sooner.
