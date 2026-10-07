### Relative globs accept //, . and .. components

#### Summary

Relative path-table and config globs now accept `//`, `.` and `..` components as bash does.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A relative path-table accepts `//`, `.` and `..` components and reports the path as written; `./docs//api.md` returns `docs//api.md`, and `./docs/../top.md` matches `top.md` through `docs/..`.

#### Verification

```bash
dirsql query "SELECT path FROM './docs/../*.md'"
```
