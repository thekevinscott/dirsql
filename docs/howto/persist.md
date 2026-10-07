# Keep the index across restarts

By default the database is ephemeral: rebuilt from your files on every
startup and discarded on exit. The [`--persist [PATH]`](../reference/cli.md#dirsql-server)
flag keeps the SQLite index on disk instead, so a restart only re-parses
files that actually changed — the difference between seconds and
milliseconds on large trees, and between re-running and skipping an expensive
declared-table [`on-file`](./extract-from-contents.md) command.

Whether and where to cache is a machine-local operational choice — it
belongs to the command you run, not to the shared `.dirsql.toml`. That is
why it is a CLI flag, not a config key.

## 1. Turn it on

```bash
dirsql --persist
```

That's the whole change. On the next run the cache is written to your platform cache directory
(`$XDG_CACHE_HOME/dirsql/<root hash>/cache.db` on Linux, `~/Library/Caches`
on macOS, `%LOCALAPPDATA%` on Windows), outside the scanned tree; runs after
that start from it. To put the cache elsewhere (a CI cache dir, a tmpfs), pass
a path:

```bash
dirsql --persist /var/cache/dirsql.db
```

The same flag works on [`dirsql query`](../reference/cli.md#dirsql-query);
put a bare `--persist` after the SQL there so it does not consume the query
argument. Embedding `dirsql`? The SDK constructors expose the same switch —
see [_Embedding `dirsql`?_](#embedding-dirsql) below.

## 2. Nothing to ignore in git

The default cache lives outside the scanned tree, so there is nothing to add
to `.gitignore` and the cache can't index itself. With `--persist <path>`, keep
that path out of the scanned tree too, or ignore it. While running, `dirsql`
also creates transient `cache.db-wal` and `cache.db-shm` sidecar files next to
the cache.

## What survives, what rebuilds

On startup `dirsql` validates the cache rather than trusting it blindly:
files whose stat metadata is unchanged keep their rows without being
re-read; changed, added, and deleted files are reconciled. When the cache
can't be trusted at all — the table/ignore configuration changed, or the
`dirsql` version did — it is discarded and rebuilt from scratch
automatically. You never need to delete it by hand; a full rebuild costs
exactly what a non-persistent startup does.

A run that changes nothing writes nothing: the cache file is read and left
exactly as it was.

This covers declared tables, which keep their indexed rows. A
[path-table](../reference/path-tables.md) is not cached: a *stat* path-table
has nothing to save, since its columns are the metadata the scan already
collected and `content` is read live by design, and a path-table parsed with
`--on-file` runs its parser on every start.

Persistence is a startup-time optimization, not a change in meaning: the
database remains a derived view of your files, and queries return the same
rows either way ([how `dirsql` thinks](../explanation.md)).

### Durability

The cache favors throughput over durability: it opens in WAL journal mode
with `synchronous=NORMAL`. On power loss the most recent cache updates may
be lost, but the file cannot corrupt — the next startup reconciles and
re-parses anything missing.

## Embedding `dirsql`?

The SDK constructors expose the same switch as `persist` / `persistPath`
parameters — see the [SDK reference](../reference/sdk.md#constructor).
