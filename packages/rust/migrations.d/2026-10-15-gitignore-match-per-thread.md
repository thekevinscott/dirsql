### Faster `.gitignore` matching

#### Summary

Each walk thread matches against its own compiled copy of a `.gitignore`. Which files a scan returns, and their order, are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
dirsql query "SELECT path FROM './**'"
```

Prints the same rows as before, sooner, inside a git repo.
