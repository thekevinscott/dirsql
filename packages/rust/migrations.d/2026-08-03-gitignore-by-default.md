### Path-table scans respect `.gitignore` by default

#### Summary

Path-table scans (`SELECT ... FROM './'`, `FROM '/abs/**'`, and the `--on-file`
parsed form) now respect `.gitignore` files by default — hierarchically, with
traversal pruning, like fd/ripgrep — in the Rust core and therefore in the CLI
and every SDK binding. A query over a repository returns fewer rows than
before when `.gitignore` names files or directories in the scanned tree. No
API signature changed; the opt-out is the new CLI flag `--no-ignore` and Rust
builder option `DirSQLBuilder::no_ignore(bool)`. Declared (config) tables are
unaffected.

#### Required changes

_None required for code._ To keep the previous full-walk behavior for a
path-table query:

| Surface | Before | After |
| ------- | ------ | ----- |
| CLI | `dirsql "SELECT * FROM './'"` (scanned gitignored files) | `dirsql "SELECT * FROM './'" --no-ignore` |
| Rust SDK | `DirSQL::builder().root(r).build()` (scanned gitignored files) | `DirSQL::builder().root(r).no_ignore(true).build()` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Path-table scans: previously `.gitignore` files were not consulted, so
  gitignored files (build output, virtualenvs, caches) appeared as rows; now
  any `.gitignore` in the scanned tree excludes its matches below its own
  directory and ignored directories are pruned. Row counts over repositories
  shrink accordingly.
- Unchanged, for scoping: hidden files are still scanned; the built-in
  `**/node_modules/**` / `**/.git/**` defaults and configured `ignore`
  patterns still apply (also under `--no-ignore`); naming an ignored
  directory outright (`FROM './dist'`) still scans it; declared tables and
  watch events are not affected.

#### Verification

```bash
mkdir -p /tmp/dirsql-gitignore-check/dist
printf 'dist/\n' > /tmp/dirsql-gitignore-check/.gitignore
touch /tmp/dirsql-gitignore-check/dist/bundle.js /tmp/dirsql-gitignore-check/app.js
cd /tmp/dirsql-gitignore-check
dirsql "SELECT path FROM './' ORDER BY path"
# expected: [{"path":".gitignore"},{"path":"app.js"}]
dirsql "SELECT path FROM './' ORDER BY path" --no-ignore
# expected: [{"path":".gitignore"},{"path":"app.js"},{"path":"dist/bundle.js"}]
```
