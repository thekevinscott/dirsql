### Backslash escapes in literal glob text

#### Summary

A `\` before any character in a path-table or config glob now quotes it in the literal directories, spelled dot-names and gitignore-exempt names too, not only inside wildcard components. Globs that matched before match the same files. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`'./do\cs/*.md'` returns `docs/api.md` where it returned nothing; `'./\.env'` returns `.env`; `'./a\.log'` returns a gitignored `a.log`. On Windows a `\` in a `./` or config glob now escapes, as on every other platform; a `\` in a drive-letter or UNC path still separates.

#### Verification

In a directory holding `docs/api.md`, run `dirsql query "SELECT path FROM './do\cs/*.md'" --format json` and expect `[{"path":"docs/api.md"}]`.
