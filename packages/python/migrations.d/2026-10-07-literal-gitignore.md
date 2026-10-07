### Named paths ignore .gitignore

#### Summary

A path the pattern spells out is listed even when a .gitignore ignores it.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A pattern with no wildcard, such as ./debug.log, lists the file even when .gitignore ignores it; wildcard matches are still filtered.

#### Verification

```bash
dirsql query "SELECT path FROM './debug.log'"
```
