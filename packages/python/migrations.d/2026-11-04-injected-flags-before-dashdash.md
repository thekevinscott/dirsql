### Launcher-injected flags go before `--`

#### Summary

The `dirsql` launcher inserts its injected `-c` (plugin fragment) and `--extension` flags before the first `--` instead of appending them, so a query passed after `--` works. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

With any dirsql plugin installed, `dirsql --format json -- "SELECT 1 AS a"` prints `[{"a":1}]` and exits 0.
