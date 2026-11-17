### Document how long a query waits while changes are being applied

#### Summary

Documentation only. `DirSQL::query`'s docs and the `dirsql context` troubleshooting guide now say that a query arriving while a batch of file changes is being applied waits for that batch, and that under a writer that never pauses it waits as long as one batch takes. An earlier entry said a query never waits on the watcher; that was wrong. Nothing breaks and no code changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql context` and find the section "A query to `dirsql server` or the REPL is slow while files are changing".
