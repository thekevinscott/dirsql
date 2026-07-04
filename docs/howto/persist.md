# Keep the index across restarts

By default the database is an anonymous temp database, rebuilt from your
files on every startup and deleted when the process exits. [`persist`](../reference/config.md#dirsql-keys) keeps the
SQLite index on disk instead, so a restart only re-parses files that
actually changed — the difference between seconds and milliseconds on large
trees, and between re-running and skipping expensive
[`on-file`](./extract-from-contents.md) commands.

## 1. Turn it on

```toml
[dirsql]
persist = true
```

That's the whole change. On the next run the cache is written to
`.dirsql/cache.db` under the root; runs after that start from it. To put
the cache elsewhere (a CI cache dir, a tmpfs), set
[`persist_path`](../reference/config.md#dirsql-keys).

## 2. Keep the cache out of git

The cache is derived data — reproducible from the tree and frequently
large. Add it to `.gitignore`:

```
.dirsql/
```

The top-level `.dirsql/` directory is reserved for `dirsql`'s metadata and
is never scanned as data, so the cache can't index itself
([config reference](../reference/config.md#dirsql-keys)).

## What survives, what rebuilds

On startup `dirsql` validates the cache rather than trusting it blindly:
files whose stat metadata is unchanged keep their rows without being
re-read; changed, added, and deleted files are reconciled. When the cache
can't be trusted at all — the table/ignore configuration changed, or the
`dirsql` version did — it is discarded and rebuilt from scratch
automatically. You never need to delete it by hand; a full rebuild costs
exactly what a non-persistent startup does.

Persistence is a startup-time optimization, not a change in meaning: the
database remains a derived view of your files, and queries return the same
rows either way ([how `dirsql` thinks](../explanation.md)).

## Embedding `dirsql`?

The SDK constructors expose the same switch as `persist` / `persistPath`
parameters — see the [SDK reference](../reference/sdk.md#constructor).
