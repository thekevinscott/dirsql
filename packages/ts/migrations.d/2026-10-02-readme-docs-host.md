### TypeScript SDK: README links point at `dirsql.dev`

**Summary**

The two documentation links in the README name `https://dirsql.dev/`, the
host that serves the docs, instead of `thekevinscott.github.io/dirsql/`, which
redirects there. README only: no code, config, or CLI surface changed.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
curl -s -o /dev/null -w '%{http_code}\n' https://dirsql.dev/reference/cli
# 200
```
