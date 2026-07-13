### The bundled `dirsql` CLI no longer auto-loads `./.dirsql.toml` (#602)

#### Summary

`npx dirsql` ships the shared Rust binary, whose config discovery changed: with
no `-c`/`--config` it now serves the baked-in default `files` table instead of
silently loading a `./.dirsql.toml` from the invocation directory. This is a
runtime-behavior change in the CLI the TypeScript package distributes; the
TypeScript SDK (`DirSQL`, `Table`, `RowEvent`, …) API is unchanged.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `npx dirsql` / `npx dirsql query "…"` against a cwd `./.dirsql.toml` | Auto-loaded | Pass it explicitly: `npx dirsql -c ./.dirsql.toml …` |
| `-c` naming a possibly-absent file | Missing default degraded silently | A missing `-c` file is an error (non-zero exit / `503`) |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Bare `npx dirsql` serves the baked-in default `files` table and ignores a cwd
  `./.dirsql.toml`; pass it with `-c` to use it.
- A `-c` naming a non-existent file errors instead of falling back to the
  default.

#### Verification

```bash
# In a directory whose ./.dirsql.toml defines a `posts` table:
npx dirsql query "SELECT COUNT(*) FROM files"              # baked-in default: succeeds
npx dirsql query "SELECT COUNT(*) FROM posts"              # errors: no such table: posts
npx dirsql -c ./.dirsql.toml query "SELECT COUNT(*) FROM posts"   # succeeds
```
