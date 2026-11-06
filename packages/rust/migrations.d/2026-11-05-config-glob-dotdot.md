### `..` in config `[[table]]` globs resolves physically

#### Summary

A config `[[table]] glob` with an absolute, `~/`, or `../` prefix that contains `..` now lets the filesystem resolve it, so a `..` after a symlinked directory climbs out of the link's target, as in the shell. No API changes; nothing breaks for globs without a symlink ahead of a `..`, except that the paths handed to the `on-file` hook, and `{root}`, keep the `..` as written.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

| Before | After |
|---|---|
| `glob = '/a/b/../*.md'` hands the hook `/a/x.md`, `{root}` is `/a` | hands it `/a/b/../x.md`, `{root}` is `/a/b/..` |
| `glob = '/a/link/../*.md'` with `link -> /real/sub` matches `/a/*.md` | matches `/real/*.md`, as `ls` does |

Windows paths keep folding `..` lexically, as Windows itself does.

#### Verification

```sh
mkdir -p r/real/sub r/y && printf P > r/real/f.md && printf L > r/y/f.md && ln -s "$PWD/r/real/sub" r/y/link
cat > t.toml <<TOML
[[table]]
name = "t"
ddl = "CREATE TABLE t (content TEXT)"
glob = '$PWD/r/y/link/../*.md'
on-file = "jq -Rc '{content: .}'"
TOML
dirsql query "SELECT content FROM t" -c t.toml
```

Expected: `content` is `P`.
