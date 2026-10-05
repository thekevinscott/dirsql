### Core: nested repositories reset `.gitignore` inheritance

#### Summary

Path-table scans no longer carry a parent repository's `.gitignore` rules
into a nested repository. This matches Git's ignore boundary behavior and can
make previously hidden files visible to path-table queries. No API changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- When `outer` is a repository with `*.log` and `outer/repo` is also a
  repository, `'./**'` now lists `repo/z.log`; the outer rule no longer
  applies inside `repo`.

#### Verification

```bash
cd "$(mktemp -d)"
git init -q
printf '*.log\n' > .gitignore
mkdir repo
git -C repo init -q
touch repo/z.log
dirsql query "SELECT path FROM './**' ORDER BY path"
# expected: [{"path":"repo/z.log"}]
```
