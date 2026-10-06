### Plugin launcher no longer seeds a `records` table

#### Summary

With a plugin installed, the `uvx`/`pip` launcher used to add the hidden `--include-default`, which seeded a placeholder `records` table (glob `**/*.json`, every row all-NULL) alongside the plugin's tables. It now injects only `-c <fragment>` per plugin. Queries that never named `records` are unaffected and no longer walk every JSON file first; a query that read the placeholder `records` now fails with `no such table: records`.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `SELECT count(*) FROM records` with a plugin installed | one row per `*.json` file | `no such table: records` |
| Want a real `records` table | (placeholder) | define it in your own config and pass it with `-c` |

#### Deprecations removed

The hidden `--include-default` flag (internal, never documented) is removed.

#### Behavior changes without code changes

- Plugin-enabled queries no longer show a `dirsql: indexing N/M files` progress line for files no plugin table matches.

#### Verification

```bash
cd "$(mktemp -d)"
echo '{"id":1}' > a.json
uvx --with dirsql-plugin-embeddings dirsql query "SELECT count(*) FROM records"
```

Expected: `no such table: records`.
