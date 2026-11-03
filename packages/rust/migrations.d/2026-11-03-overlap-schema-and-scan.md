### Overlap table creation with startup scan

#### Summary

Transient startup builds create their SQLite schema while scanning files. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

In a directory with a `.dirsql.toml` declaring a `notes` table and one matching file, run `dirsql query 'SELECT count(*) AS n FROM notes' -c .dirsql.toml`; the output contains `"n": 1`.
