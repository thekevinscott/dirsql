### Faster path rows

#### Summary

Path queries over large trees build, read and free their rows with fewer allocations. Results and ordering are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql --help`; the CLI starts as before.
