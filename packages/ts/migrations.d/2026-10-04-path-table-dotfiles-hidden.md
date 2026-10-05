### Core: path-tables hide dot-named entries unless the path spells them

#### Summary

A path-table scan now skips any file or directory whose name starts with `.`
unless that component is spelled in the table's path -- the rule `ls` and
`fd` use. It used to list dotfiles like any other file. Every surface that
resolves a path-table is affected: the CLI, the REPL, the HTTP server, the
SDKs, and the `--on-file` form. A query that relied on `'./'` or `'./**'`
reaching `.gitignore`, `.env` or a `.claude/` tree must name the dot
component. Declared `[[table]]` globs in `.dirsql.toml` are unchanged.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| A dot directory's files | `SELECT * FROM './**'` (rows under `.claude/` included) | `SELECT * FROM './.claude/**'` |
| One dotfile | `SELECT * FROM './*'` (`.env` included) | `SELECT * FROM './.env'` |
| Dotfiles at any depth | `SELECT * FROM './**'` | `SELECT * FROM './**/.env'` |
| Every dot-named entry, one level | `SELECT * FROM './*'` | `SELECT * FROM './.*'` |
| Spelled dot prefix (unchanged) | `SELECT * FROM '~/.claude/projects/*/*.jsonl'` | `SELECT * FROM '~/.claude/projects/*/*.jsonl'` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./'`, `'./*'`, `'./**'` and every other path-table whose path spells no
  dot component return no dot-named files and do not walk dot-named
  directories, at any depth.
- `--no-ignore` (`no_ignore` / `noIgnore` in the SDKs) restores gitignored
  files only; dot-named entries stay hidden under it.

#### Verification

```ts
// root holds top.md, .env and .claude/notes.md
await new DirSQL({ root }).query("SELECT path FROM './**' ORDER BY path");
// expected: [{ path: "top.md" }]
await new DirSQL({ root }).query("SELECT path FROM './.claude/**'");
// expected: [{ path: ".claude/notes.md" }]
await new DirSQL({ root }).query("SELECT path FROM './.env'");
// expected: [{ path: ".env" }]
```
