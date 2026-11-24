### The live watcher tracks glob-named files under gitignored directories

#### Summary

The live event filter now exempts a gitignored path a table glob spells out, as the startup scan does, so such a file's edits and deletion reach the index. Nothing breaks and no code changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

With `.gitignore` listing `build/` and a table globbing `build/x.md`, deleting or editing `build/x.md` while watching now removes or updates its row; before, the row stayed as the startup scan left it.

#### Verification

In a git repo whose `.gitignore` lists `build/`, run `dirsql --watch` with one table globbing `build/x.md`, delete `build/x.md`, and see a delete event for it.
