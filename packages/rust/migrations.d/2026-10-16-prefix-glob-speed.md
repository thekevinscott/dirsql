### Prefix glob speed

#### Summary

One-shot CLI queries start fewer threads, and the walk starts its workers as a tree. No API changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT path FROM './a/b/**/*.md'"
```

Prints the same rows as before, sooner.
