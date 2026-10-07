### Core: a spelled dot component no longer opens dot directories above it

#### Summary

A path-table glob whose dot component sits below `**`, such as `'./**/.*'`,
used to descend into every dot-named directory and return the dotfiles
inside. It now enters a dot-named directory only where a component spells it
at that depth, matching bash and the path-tables reference. Queries on such
globs can return fewer rows.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./**/.*'` no longer lists `.cache/.z`; it still lists `.x` and `a/.y`.
- To read inside a dot-named directory, spell it: `'./.cache/**/.*'`.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p a .cache
touch .x a/.y .cache/.z
dirsql query "SELECT path FROM './**/.*' ORDER BY path"
# expected: [{"path":".x"},{"path":"a/.y"}]
```
