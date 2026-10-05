### Core: path-tables apply `.gitignore` files above the scan start

#### Summary

A path-table scan used to read only the `.gitignore` files at or below the
directory it starts in, so `'./docs/*.log'` listed files a root `*.log` rule
hides from `'./**'`. Inside a git repo it now reads every `.gitignore` from the
repo root (the nearest directory holding `.git`) down to that directory. Every
surface that resolves a path-table is affected: the CLI, the REPL, the HTTP
server, the SDKs, and the `--on-file` form. No call changes; a query can return fewer rows. `--no-ignore` restores
the old listing.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./docs/*.log'` no longer lists `docs/z.log` when a root `.gitignore`
  ignores `*.log`.
- Run from a subdirectory of a repo, path-tables honor the `.gitignore` files
  between the repo root and that subdirectory.

#### Verification

```bash
cd "$(mktemp -d)"
git init -q
mkdir docs
printf '*.log\n' > .gitignore
touch docs/a.md docs/z.log
dirsql query "SELECT path FROM './docs/*' ORDER BY path"
# expected: [{"path":"docs/a.md"}]
```
