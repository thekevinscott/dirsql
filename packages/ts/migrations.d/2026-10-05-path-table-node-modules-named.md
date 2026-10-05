### Core: `node_modules` named after a glob is scanned

#### Summary

A path-table pattern with a literal `node_modules` component after a glob,
such as `'./**/node_modules/**'`, now scans the `node_modules` directories it
names; it used to return nothing. Every surface that resolves a path-table is
affected: the CLI, the REPL, the HTTP server, the SDKs, and the `--on-file`
form. Nothing breaks; such a query can return more rows. A pattern that does
not name `node_modules` still skips it, and declared `[[table]]` globs are
unchanged.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./**/node_modules/**'` lists files under every `node_modules`.
- `'./*/node_modules/*/*'` lists files two levels into each `node_modules`
  one level down.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p node_modules/pkg pkg/node_modules/dep
touch node_modules/pkg/index.js pkg/node_modules/dep/i.js
dirsql query "SELECT path FROM './**/node_modules/*/*' ORDER BY path"
# expected: [{"path":"node_modules/pkg/index.js"},{"path":"pkg/node_modules/dep/i.js"}]
dirsql query "SELECT path FROM './**/*.js'"
# expected: []
```
