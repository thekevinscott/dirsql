### Single-pass startup scan

#### Summary

Startup scans process file entries in one pass. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql query "SELECT count(*) AS n FROM './notes/**/*.md'"` in a directory with Markdown notes; the count remains the number of matching files.
