### Dot names hidden below a `.*` component

#### Summary

A dot-named file or directory is listed only where a glob component spelled with a leading `.` sits at that depth, as in bash `-O globstar`, for path-tables and declared tables alike. Before, a `.*` component anywhere in the glob admitted dot-named files at any depth. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A glob such as `.*/*` or `.*/**` no longer returns dot-named files below the first level. Spell the dot component at the depth you want, e.g. `.*/.*`.

#### Verification

In an empty directory, create `.cfg/app.toml`, `.cfg/.secret` and `.cfg/sub/.cache`, then run `dirsql query "SELECT path FROM './.*/**' ORDER BY path" --format json`; the output is `[{"path":".cfg/app.toml"}]`.
