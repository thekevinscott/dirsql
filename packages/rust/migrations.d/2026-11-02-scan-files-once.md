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

In a directory containing exactly one file, `notes/one.md`, run `dirsql query "SELECT count(*) AS n FROM './notes/**/*.md'"`; the output contains `"n": 1`.
