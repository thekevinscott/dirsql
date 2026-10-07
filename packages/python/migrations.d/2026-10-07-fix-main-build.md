### Build fix

#### Summary

Fixes the build break from two merged changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- No user-visible change.

#### Verification

```bash
dirsql query "SELECT path FROM './docs/../*.md'"
```
