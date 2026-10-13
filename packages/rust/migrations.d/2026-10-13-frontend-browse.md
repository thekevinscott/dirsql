### Frontend table browsing and onboarding

#### Summary

The `dirsql server --frontend` page now shows the query root, documents path-table columns, lists declared tables with their columns, and loads a quoted, bounded preview query when a table is chosen. A config that fails to load shows its diagnostic and how to restart. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

Run `dirsql server --frontend --port 0 -c .dirsql.toml` in a directory with a config, open the printed URL, and choose a table. The SQL box reads `SELECT * FROM "<table>" LIMIT 100`; press Run to see its rows.
