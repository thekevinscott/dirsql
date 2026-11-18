### The watcher watches only directories a table glob can reach

#### Summary

The live watcher no longer adds a watch on every directory under each table anchor; it watches only the directories a table glob can reach, and the directories leading to them, as the scan prunes its walk. Nothing breaks and no code changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On Linux, a tree whose table globs reach few of its directories uses that many inotify watches, not one per directory, so a build that failed with `inotify watch limit reached` may now succeed without raising `fs.inotify.max_user_watches`.

#### Verification

On Linux, in a tree with many directories outside `docs/`, run `dirsql --watch` (or build a watching `DirSQL`) with one table globbing `docs/**/*.md`, then count the watches: `grep -c '^inotify wd:' /proc/<pid>/fdinfo/<inotify fd>` reports one per directory under `docs/` plus the root.
