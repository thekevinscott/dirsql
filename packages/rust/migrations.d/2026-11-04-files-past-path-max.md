### Files past the path limit are path-table rows

#### Summary

A path table now lists every file under its root, including files whose absolute path is longer than the platform's path limit, with their stat columns and content. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On Unix, a query over a tree nested deeper than `PATH_MAX` returns the files below that depth, where it silently returned none of them before.

#### Verification

In an empty directory, run `n=$(printf 'D%.0s' $(seq 200)); ( for i in $(seq -w 1 30); do mkdir "$n"; cd "$n"; printf x > "leaf$i.md"; done )`, then `dirsql "SELECT COUNT(*) n FROM './**'" --format json`: it prints `[{"n":30}]`.
