### Core: `pre-query` / `post-query` hooks removed

#### Summary

The server-wide `pre-query` and `post-query` command hooks are deleted from
the core (#803, part of the #800 plugin redesign — their only motivating
consumer was the old `dirsql-plugin-embeddings` surface, removed in #802).
Every install channel is affected identically (`pip` / `npm` / `cargo`, CLI
and HTTP server): the `[dirsql].pre-query` / `[dirsql].post-query` config keys
are no longer part of the schema, and the Rust `cli::PreQuery` /
`cli::PostQuery` types and `ServerConfig::with_pre_query` /
`with_post_query` builders no longer exist. A config carrying either key now
fails to load with the standard unknown-key error naming the key. `on-file`,
`[[table]]`, and `hook-timeout` (which still bounds `on-file` runs) are
unchanged.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `[dirsql].pre-query` config key | `pre-query = "to_sql.py {args}"` rewrote each `POST /query` body into SQL | Remove the key (a config carrying it fails with `unknown field \`pre-query\``); send `{"sql": …}` bodies and do any rewriting in the client before the request |
| `[dirsql].post-query` config key | `post-query = "jq -c '{results: .}'"` reshaped each result set | Remove the key (unknown-key error); reshape the returned row array in the client (e.g. pipe `dirsql query` output through `jq`) |
| Rust `cli::ServerConfig` | `ServerConfig::ephemeral().with_pre_query(PreQuery::new(cmd, dir))` / `.with_post_query(PostQuery::new(cmd, dir))` | Types and builders removed; construct `ServerConfig` without hook stages |

#### Deprecations removed

_None._ (The keys were removed outright, with no deprecation period.)

#### Behavior changes without code changes

- `POST /query` (and `dirsql query`): the request body is now always parsed
  as `{"sql": …}` and a successful response is always the bare row array —
  there is no config that can intercept either side.
- A `.dirsql.toml` containing `pre-query` or `post-query`: previously loaded
  and armed the hooks; now the server degrades (503 naming the key) and
  `dirsql query` exits non-zero naming the key on stderr.

#### Verification

```bash
printf '[dirsql]\npre-query = "cat"\n' > /tmp/hooked.toml
dirsql query "SELECT 1 AS one" -c /tmp/hooked.toml
# expected: non-zero exit; stderr contains `unknown field `pre-query``

dirsql query "SELECT 1 AS one"
# expected: [{"one":1}]
```
