### ../ paths as written

#### Summary

A ../ path-table reports paths as written.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A ../ path-table such as '../notes/*.md' now reports ../notes/a.md, relative to the directory dirsql runs in, instead of an absolute path. / and ~/ tables are unchanged.

#### Verification

```bash
dirsql query "SELECT path FROM '../notes/*.md'"
```
