### Persist cache switches to WAL journal mode (#598)

#### Summary

The persist cache is now opened with `journal_mode=WAL` and `synchronous=NORMAL` to eliminate per-commit fsyncs and reduce ~100x write amplification on slow storage. This affects all three SDKs and the CLI through the shared core. The API and config surface remain unchanged; the modification is internal to how SQLite is configured on the persistent database file.

#### Required changes

_None._ (No API, config, or CLI change.)

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `cache.db-wal` and `cache.db-shm` sidecars exist beside the cache while a process holds it open. Tooling that copies a **live** cache must copy all three files together (a cleanly closed cache remains a single `cache.db` file, as before).
- Durability: power loss may drop the most recent cache updates — the file never corrupts. The next startup reconciles and re-parses whatever the cache is missing.
- The journal mode persists in the database file header: a cache touched by this version stays in WAL mode when later opened by an older `dirsql` (harmless; SQLite ≥ 3.7.0 understands WAL).

#### Verification

Run a persist build:

```bash
dirsql --persist
# or:
dirsql query --persist 'SELECT * FROM rows' <<EOF
file=test.json
EOF
```

Check the journal mode:

```bash
sqlite3 .dirsql/cache.db 'PRAGMA journal_mode;'
# Expected output: wal
```
