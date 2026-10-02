### dirsql-plugin-embeddings: vector cache moves to SQLite, worker requests are batched

#### Summary

The on-disk vector cache changes format: `~/.cache/dirsql/embeddings/` (or
`$XDG_CACHE_HOME/dirsql/embeddings/`) now holds one SQLite database per model
identifier instead of one `cachetta` file per cached value. Nothing reads the
old files. The first query after upgrading re-embeds everything it touches;
later runs hit the new store. The worker protocol gains the batched
`{"calls": [...]}` request alongside the single `{"call": [...]}` request,
and the packaged fragment opts in with `batch = 4096`, which needs dirsql
0.4.60 or later.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Cache directory contents | One `cachetta` file per cached value | One `<sha256 of model identifier>.db` SQLite database per model; old files are dead weight — wipe the directory to reclaim their space |
| `dirsql` version with the packaged fragment | Any release with `[[dirsql.function]]` | 0.4.60 or later (`batch` is rejected as an unknown field by older releases) |
| Worker protocol | `{"call": [value, model_id?]}` only | Also `{"calls": [[value, model_id?], ...]}` answered by `{"results": [...]}` in request order |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- The first query after upgrading reports every `embed()` call as computed
  rather than cached, even over files embedded before, because the old
  per-value files are never read.
- A statement's `embed()` values reach the worker in batches of up to 4096;
  a failing model fails every call in its batch group with the same error
  instead of one call at a time.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'hello world\n' > note.md
uvx --with dirsql-plugin-embeddings dirsql query "SELECT path, vec_length(embed(content)) AS dims FROM '*.md'"
# expected: [{"path":"note.md","dims":512}]
ls "${XDG_CACHE_HOME:-$HOME/.cache}/dirsql/embeddings/"
# expected: exactly one <64 hex chars>.db file for the default model
```
