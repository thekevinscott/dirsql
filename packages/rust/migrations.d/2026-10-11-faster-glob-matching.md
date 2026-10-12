### Faster glob matching

#### Summary

Glob matching under `**` copies fewer paths. Results and ordering are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql --help`; the CLI starts as before.
