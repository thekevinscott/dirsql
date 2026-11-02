### Faster per-table rerun reconcile

#### Summary

A per-table `on-file` rerun after a watched change does less SQLite work before its events are sent. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
mkdir -p d
echo '{"v": 0}' > d/f.jl
printf '[[table]]\nname = "rows"\nglob = "**/*.jl"\nddl = "CREATE TABLE rows (v INTEGER)"\non-file = "cat"\n' > .dirsql.toml
dirsql server -c .dirsql.toml --port 7117 &
curl -sN http://localhost:7117/events &
echo '{"v": 1}' > d/f.jl
```

Prints a `rows` event carrying `"row":{"v":1}`.
