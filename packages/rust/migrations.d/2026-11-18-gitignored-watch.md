### The watcher skips gitignored directories

#### Summary

The live watcher now skips the directories `.gitignore` hides, as the startup scan does, and re-walks from a `.gitignore` that changes while watching so the directories it un-ignores are watched and indexed. Nothing breaks and no code changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On Linux and the BSDs, a gitignored directory a table glob reaches costs no watch, and one a `.gitignore` edit un-ignores while watching gets one. On Linux, that edit also indexes the files already in the directory; before, only files written there afterward were picked up. Elsewhere, files already in an un-ignored directory are indexed at the next build.

#### Verification

On Linux, in a git repo whose `.gitignore` lists `build/`, run `dirsql --watch` with one table globbing `**/*.md`, then count the watches: `grep -c '^inotify wd:' /proc/<pid>/fdinfo/<inotify fd>` counts none under `build/`.
