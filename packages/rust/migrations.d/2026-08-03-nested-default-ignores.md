### Default path-table ignores apply at any depth (#741)

#### Summary

The built-in path-table skip rules were anchored at the table root
(`node_modules/**`, `.git/**`), so a `node_modules` or `.git` nested below a
subdirectory leaked into path-table query results. The defaults are now
`**/node_modules/**` and `**/.git/**`, matching at any depth (globset's
`**/foo` also matches a top-level `foo`, so root-level behavior is
unchanged). No API signature changes — this changes query results only.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Path-table scans (`FROM './'`, `FROM './dir'`, absolute and `~/` forms):
  previously files under a *nested* `node_modules/` or `.git/` (e.g.
  `apps/site/node_modules/pkg/index.js`) appeared in results; now they are
  skipped like their top-level counterparts. Queries that relied on nested
  `node_modules`/`.git` contents surfacing implicitly must name the
  directory outright (`FROM './apps/site/node_modules'`), which still scans
  it — the skip rules continue to apply only beneath the literal path you
  write.
- The walker now prunes wholly-ignored directories during traversal instead
  of filtering matches afterwards, so ignored trees are no longer read at
  all. Results are identical apart from the nested-skip fix; large ignored
  trees simply scan faster.

#### Verification

```bash
mkdir -p /tmp/d741/sub/node_modules
echo x > /tmp/d741/sub/node_modules/x.txt
echo y > /tmp/d741/sub/real.txt
cd /tmp/d741
dirsql "SELECT path FROM './'"
# expected: [{"path":"sub/real.txt"}]  (no node_modules row)
dirsql "SELECT path FROM './sub/node_modules'"
# expected: [{"path":"sub/node_modules/x.txt"}]  (named outright, still scans)
```
