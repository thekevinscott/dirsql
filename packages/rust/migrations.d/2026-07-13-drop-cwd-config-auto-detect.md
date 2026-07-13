### Bare `dirsql` no longer auto-loads `./.dirsql.toml` (#602)

#### Summary

The CLI previously synthesized `./.dirsql.toml` as the config path when no
`-c`/`--config` was given, silently loading a config that happened to sit in
the invocation directory. That implicit on-disk discovery is removed: "default
config" now means the **baked-in shipped config** (the single `files` table,
`DEFAULT_CONFIG_TOML`), which is invisible to users and always the same. This
affects the CLI (`dirsql` server mode and `dirsql query`) shipped in the Rust
crate and in the `pip` / `npm` launchers; it does **not** touch any SDK
signature (the `DirSQLBuilder` shortcut removal is a separate change, #603).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Run against a `./.dirsql.toml` in the current directory | `dirsql` / `dirsql query "…"` (auto-loaded) | `dirsql -c ./.dirsql.toml` / `dirsql -c ./.dirsql.toml query "…"` — pass it explicitly |
| Rely on the zero-config `files` table | `dirsql` (in a directory with no `.dirsql.toml`) | `dirsql` — unchanged; the baked-in default is served regardless of what is on disk |
| Point `-c` at a file that may be absent | Missing default silently degraded to the `files` table | A missing `-c` file is an error: `dirsql query` exits non-zero naming the file; server mode binds degraded and returns `503` |
| Scaffold a config with `dirsql init` | `dirsql init` then `dirsql` (auto-loaded the written file) | `dirsql init` then `dirsql -c ./.dirsql.toml` — the output no longer auto-loads |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Config discovery**: with no `-c`, `dirsql` serves the baked-in default
  `files` table and never consults a cwd `./.dirsql.toml`. A present
  `./.dirsql.toml` is ignored unless passed with `-c`.
- **Missing explicit config**: a `-c <path>` naming a non-existent file no
  longer falls back to the default. `dirsql query` exits non-zero with the path
  on stderr; server mode returns `503` for `/query` and `/events`.
- `dirsql init` output is a scaffold you load explicitly with `-c`; writing it
  no longer changes what a subsequent bare `dirsql` serves.

#### Verification

In a directory containing a `.dirsql.toml` that defines a table named `posts`:

```bash
# Baked-in default is served; the on-disk `posts` table is NOT auto-loaded:
dirsql query "SELECT COUNT(*) FROM files"   # succeeds
dirsql query "SELECT COUNT(*) FROM posts"   # errors: no such table: posts

# Pass the config explicitly to use it:
dirsql -c ./.dirsql.toml query "SELECT COUNT(*) FROM posts"   # succeeds

# A missing -c config is an error, not a fallback:
dirsql -c ./missing.toml query "SELECT 1"   # exits non-zero, names ./missing.toml
```
