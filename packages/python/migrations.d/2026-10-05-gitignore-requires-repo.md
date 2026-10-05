### Python SDK: `.gitignore` applies only inside a git repo

#### Summary

Path-tables used to apply `.gitignore` files with no git repo present. Now a
`.gitignore` is in force only when a directory holding `.git` encloses it,
as in git, fd and ripgrep. Every surface that resolves a path-table is
affected: the CLI, the REPL, the HTTP server, the SDKs, and the `--on-file`
form. No call changes. Outside a repo a query can return more rows; inside
one nothing changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Outside a git repo, files a `.gitignore` names are listed. To hide them,
  add `ignore` patterns in a config file, or run `git init`.

#### Verification

```bash
cd "$(mktemp -d)"
printf '*.log\n' > .gitignore
touch a.md z.log
dirsql query "SELECT path FROM './*' ORDER BY path"
# expected: [{"path":"a.md"},{"path":"z.log"}]
git init -q
dirsql query "SELECT path FROM './*' ORDER BY path"
# expected: [{"path":"a.md"}]
```
