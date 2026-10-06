### Config tables respect .gitignore

#### Summary

A declared `[[table]]` used to read every file its glob matched, whatever
`.gitignore` said. It now skips gitignored files, as a path-table does. This
breaks a config that relied on indexing an ignored file; pass `--no-ignore`
(or `no_ignore` in the SDKs) to restore the old behavior.

#### Required changes

| Before | After |
| --- | --- |
| a table indexed gitignored files | pass `--no-ignore`, or stop ignoring the file |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Inside a git repo (a directory holding `.git`), files a `.gitignore` ignores
  are not rows of a `[[table]]`, at startup or while watching.
- Outside a repo nothing changes.

#### Verification

```bash
cd "$(mktemp -d)"
git init -q
printf 'skip.json\n' > .gitignore
echo '[{"p":"keep"}]' > keep.json
echo '[{"p":"skip"}]' > skip.json
cat > .dirsql.toml <<'TOML'
[[table]]
name = "t"
glob = "*.json"
ddl = "CREATE TABLE t (p TEXT)"
on-file = "jq -sc add"
TOML
dirsql query "SELECT p FROM t" -c .dirsql.toml
# expected: [{"p":"keep"}]
dirsql query "SELECT p FROM t ORDER BY p" -c .dirsql.toml --no-ignore
# expected: [{"p":"keep"},{"p":"skip"}]
```
