### Core: `*` stops at `/` in `[[table]] glob` and `[dirsql] ignore`

#### Summary

Declared-table globs and ignore patterns were compiled with `globset`
defaults, where `*` and `?` also match `/`, so `glob = "*.json"` quietly
selected `.json` files at every depth and `ignore = ["folder/*"]` hid the
whole `folder/` subtree. Path-tables already compiled with
`literal_separator`, so one spelling meant two things. Every surface now
compiles the same way: `*` and `?` match within one path segment, `**` matches
any depth. Affected: the `glob` key of a `[[table]]`, the `[dirsql] ignore`
list, and the `tables` / `ignore` builder parameters of the Rust, Python and
TypeScript SDKs, which share this matcher.

#### Required changes

A pattern that relied on `*` crossing `/` must say `**`.

| Surface | Before | After |
| ------- | ------ | ----- |
| `[[table]] glob`, every depth | `glob = "*.json"` | `glob = "**/*.json"` |
| `[[table]] glob`, a subtree | `glob = "folder/*"` | `glob = "folder/**"` |
| `[dirsql] ignore`, a subtree | `ignore = ["folder/*"]` | `ignore = ["folder/**"]` |
| `[dirsql] ignore`, an extension at every depth | `ignore = ["*.tmp"]` | `ignore = ["**/*.tmp"]` |
| SDK `tables` / `ignore` parameters | same patterns | same rewrites |

A pattern already written with `**` (`**/*.md`, `folder/**`, the built-in
`**/node_modules/**` and `**/.git/**`) matches exactly what it did before. A
pattern meant to stay flat (`glob = "data/*.csv"` over a flat `data/`) needs
no change.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `glob = "*"` matches the files directly inside the root; it used to match
  every file at every depth.
- `glob = "folder/*"` matches `folder/a.json` but no longer `folder/sub/b.json`.
- `ignore = ["*"]` hides the top-level files only; `ignore = ["folder/*"]`
  hides `folder/a.json` but no longer `folder/sub/b.json`.
- `?` matches one character that is not `/`.
- A `{name}` placeholder matches exactly one path segment, as documented.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p folder/sub
printf '{}' > root.json
printf '{}' > folder/a.json
printf '{}' > folder/sub/b.json
cat > .dirsql.toml <<'TOML'
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.json"
on-file = '''sh -c 'printf "["; sep=""; for p; do printf "%s{\"path\":\"%s\"}" "$sep" "$p"; sep=","; done; printf "]"' sh'''
TOML
dirsql query "SELECT count(*) AS n FROM files" -c .dirsql.toml
# expected: [{"n":1}]   (root.json only; before: 3)
sed -i 's|glob = "\*.json"|glob = "**/*.json"|' .dirsql.toml
dirsql query "SELECT count(*) AS n FROM files" -c .dirsql.toml
# expected: [{"n":3}]
```
