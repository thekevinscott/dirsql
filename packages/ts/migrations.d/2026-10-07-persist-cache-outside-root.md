### Default persist cache moves to the platform cache directory

#### Summary

Bare `--persist` (and `.persist(None)` / `persist=True` / `persist: true` in the
SDKs) no longer writes `<root>/.dirsql/cache.db`. It writes
`cache.db` under the platform cache directory, keyed by a hash of the root. The
scan no longer reserves `.dirsql/`.

#### Required changes

| Before | After |
| --- | --- |
| `.gitignore` entry `.dirsql/` for the cache | Not needed |
| `rm -rf <root>/.dirsql` to reset the cache | Delete the file under the platform cache directory, or pass `--persist <path>` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- The first `--persist` run after upgrading rebuilds the cache once.
- An old `<root>/.dirsql/cache.db` is left behind and, being an ordinary dot directory, is now scannable by a glob that spells `.dirsql`. Delete it.
- A top-level `.dirsql/` directory is no longer excluded from the scan.

#### Verification

```bash
cd "$(mktemp -d)"
echo 'a' > a.txt
dirsql query "SELECT path FROM './*'" --persist
ls -A
# expected: only a.txt; no .dirsql/ in the root
```
