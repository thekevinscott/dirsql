### Path-table scans honor `.git/info/exclude` and `core.excludesFile`

#### Summary

Inside a git repo, path-table scans now hide what `$GIT_DIR/info/exclude` and the global `core.excludesFile` name, as git does. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A file listed only in `.git/info/exclude` or the global excludes file no longer appears in a wildcard path-table scan. Pass `--no-ignore` to list it again.

#### Verification

In a git repo holding `a.txt` and `b.txt`, run `echo a.txt >> .git/info/exclude`, then `dirsql query "SELECT path FROM './*'" --format json`: it prints `[{"path":"b.txt"}]`.
