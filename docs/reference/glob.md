# Globs

A glob is the string you write where a table name goes (`FROM './docs/*.md'`)
or in a config `[[table]]`'s `glob`. dirsql reads it the way `bash` does with
`globstar` on and `dotglob` off, and every place it does not is listed under
[Divergences from bash](#divergences-from-bash).

## Why bash

Bash with `globstar` is the most popular glob behavior. Bash, zsh, node-glob
and minimatch (npm, ESLint, VS Code, GitHub Actions) and Python's `glob` agree
on one core: `*`, `?` and `[...]`, recursive `**`, `{a,b}` braces, `*` hiding
dotfiles, and `**` not entering symlinked directories. Python has no braces
and follows symlinks; bash breaks that tie. POSIX has no `**` or braces. So
there is one standard nobody has to learn, the same string works in a
[path-table](./path-tables.md) and in a [config](./config.md) `glob`, and each
divergence is a written decision.

## The fixture

Every example below runs against this tree. A name ending in `/` is an empty
directory, `a -> b` is a symlink, and `.gitignore` holds `*.log`.

<!-- conformance-tree -->
```text
top.md
a.md
b.md
c.md
1.md
2.md
3.md
4.md
x.txt
ignored.log
{q}.md
[a].md
[z.md
my file.md
.env
.git/
.config/app.toml
.config/.secret
.config/sub/deep.md
docs/guide.md
docs/api.md
docs/.draft.md
docs/nested/deep.md
docs/nested/.cache/x.md
node_modules/pkg/index.js
real/r.md
uni/1.md
uni/a.md
uni/é.md
uni/É.md
uni/中.md
uni/٣.md
uni/¡.md
linkdir -> real
link.md -> top.md
broken -> nowhere
```

## Examples

Each row lists the files the pattern returns, sorted. Every row without a
divergence was produced by `bash -O globstar -O nullglob` with `dotglob` off,
run from the fixture's root, keeping only files. The `Divergence` column names
the rule from [below](#divergences-from-bash) that makes a row differ from
bash's raw output. `$ROOT` is the fixture's directory and `$BASE` its parent.

A test runs every row against dirsql as a path-table and, with the leading
`./` dropped, as a config `glob`, so this table cannot drift from the code.

<!-- conformance-table -->
| Pattern | Rows | Divergence |
| --- | --- | --- |
| `./*.md` | `1.md`, `2.md`, `3.md`, `4.md`, `[a].md`, `[z.md`, `a.md`, `b.md`, `c.md`, `link.md`, `my file.md`, `top.md`, `{q}.md` |  |
| `./**/*.md` | `1.md`, `2.md`, `3.md`, `4.md`, `[a].md`, `[z.md`, `a.md`, `b.md`, `c.md`, `docs/api.md`, `docs/guide.md`, `docs/nested/deep.md`, `link.md`, `linkdir/r.md`, `my file.md`, `real/r.md`, `top.md`, `uni/1.md`, `uni/a.md`, `uni/¡.md`, `uni/É.md`, `uni/é.md`, `uni/٣.md`, `uni/中.md`, `{q}.md` |  |
| `./**` | `1.md`, `2.md`, `3.md`, `4.md`, `[a].md`, `[z.md`, `a.md`, `b.md`, `c.md`, `docs/api.md`, `docs/guide.md`, `docs/nested/deep.md`, `link.md`, `my file.md`, `node_modules/pkg/index.js`, `real/r.md`, `top.md`, `uni/1.md`, `uni/a.md`, `uni/¡.md`, `uni/É.md`, `uni/é.md`, `uni/٣.md`, `uni/中.md`, `x.txt`, `{q}.md` |  |
| `./?.md` | `1.md`, `2.md`, `3.md`, `4.md`, `a.md`, `b.md`, `c.md` |  |
| `./[ab].md` | `a.md`, `b.md` |  |
| `./[!a].md` | `1.md`, `2.md`, `3.md`, `4.md`, `b.md`, `c.md` |  |
| `./\[a\].md` | `[a].md` |  |
| `./[z.md` | `[z.md` |  |
| `./uni/[[:alpha:]].md` | `uni/a.md`, `uni/É.md`, `uni/é.md`, `uni/٣.md`, `uni/中.md` |  |
| `./uni/[[:alnum:]].md` | `uni/1.md`, `uni/a.md`, `uni/É.md`, `uni/é.md`, `uni/٣.md`, `uni/中.md` |  |
| `./uni/[[:lower:]].md` | `uni/a.md`, `uni/é.md` |  |
| `./uni/[[:upper:]].md` | `uni/É.md` |  |
| `./uni/[[:digit:]].md` | `uni/1.md` |  |
| `./uni/[[:punct:]].md` | `uni/¡.md` |  |
| `./uni/[[:word:]].md` | `uni/1.md`, `uni/a.md`, `uni/É.md`, `uni/é.md`, `uni/٣.md`, `uni/中.md` |  |
| `./uni/[![:alpha:]].md` | `uni/1.md`, `uni/¡.md` |  |
| `./[d/x]ocs/api.md` | none |  |
| `./docs[/]api.md` | none |  |
| `./[!/]*/api.md` | none |  |
| `./{a,b}.md` | `a.md`, `b.md` |  |
| `./{q}.md` | `{q}.md` |  |
| `./{a.md` | none |  |
| `./{1..3}.md` | `1.md`, `2.md`, `3.md` |  |
| `./my file.md` | `my file.md` |  |
| `./.*` | `.env`, `.gitignore` |  |
| `./**/.*` | `.env`, `.gitignore`, `docs/.draft.md` |  |
| `./.env` | `.env` |  |
| `./.config/**` | `.config/app.toml`, `.config/sub/deep.md` |  |
| `./docs/.*` | `docs/.draft.md` |  |
| `./link.md` | `link.md` |  |
| `./linkdir/*` | `linkdir/r.md` |  |
| `./*/r.md` | `linkdir/r.md`, `real/r.md` |  |
| `./**/r.md` | `linkdir/r.md`, `real/r.md` |  |
| `./real/**` | `real/r.md` |  |
| `./docs//api.md` | `docs//api.md` |  |
| `./docs//*.md` | `docs//api.md`, `docs//guide.md` |  |
| `./*//api.md` | `docs/api.md` |  |
| `./docs/./api.md` | `docs/./api.md` |  |
| `./*/./api.md` | `docs/./api.md` |  |
| `./docs/../top.md` | `docs/../top.md` |  |
| `./docs/nested/../../top.md` | `docs/nested/../../top.md` |  |
| `./[dlr]*/../top.md` | `docs/../top.md`, `linkdir/../top.md`, `real/../top.md` |  |
| `./**/nested/../api.md` | `docs/nested/../api.md` |  |
| `./docs/*/../api.md` | `docs/nested/../api.md` |  |
| `./docs` | `docs/api.md`, `docs/guide.md` | directory-name |
| `./docs/` | `docs/api.md`, `docs/guide.md` | directory-name |
| `./*/` | `docs/api.md`, `docs/guide.md`, `linkdir/r.md`, `real/r.md`, `uni/1.md`, `uni/a.md`, `uni/¡.md`, `uni/É.md`, `uni/é.md`, `uni/٣.md`, `uni/中.md` | directory-name |
| `./d*` | none | files-only |
| `./*.log` | none | gitignore |
| `./ignored.log` | `ignored.log` |  |
| `./{ignored,x}.log` | `ignored.log` |  |
| `./**/*.js` | `node_modules/pkg/index.js` |  |
| `./node_modules/*/index.js` | `node_modules/pkg/index.js` |  |
| `./docs/**/*.md` | `docs/api.md`, `docs/guide.md`, `docs/nested/deep.md` |  |
| `./docs/*.md` | `docs/api.md`, `docs/guide.md` |  |
| `$ROOT/docs/*.md` | `$ROOT/docs/api.md`, `$ROOT/docs/guide.md` |  |
| `~/docs/*.md` | `$ROOT/docs/api.md`, `$ROOT/docs/guide.md` |  |
| `../proj/docs/*.md` | `../proj/docs/api.md`, `../proj/docs/guide.md` |  |
| `../*/top.md` | `../proj/top.md` |  |
| `../proj/**/deep.md` | `../proj/docs/nested/deep.md` |  |
| `../proj/docs` | `../proj/docs/api.md`, `../proj/docs/guide.md` | directory-name |

## Divergences from bash

| Tag | Rule | Why |
| --- | --- | --- |
| `gitignore` | Inside a git repo, `.gitignore` files hide files and directories a wildcard matches, never one the pattern spells out. `--no-ignore` turns it off. | A query over a repo should not drown in build output. |
| `directory-name` | A pattern's last component that names a directory lists the files directly inside it, like `ls docs`. A trailing `/` is `*` appended. | A table is rows of files, and `ls` is the shell habit. |
| `files-only` | A pattern that matches a directory but no file returns nothing. | Every row is a file. |

`.git` needs no rule of its own: the dotfile rule hides it.

## Where the glob is anchored

A path-table pattern is relative to the index root and starts with `./`. A config `[[table]] glob`
is relative to the directory holding its config file, whatever directory you
run `dirsql` from, and is written without the `./`: `./docs/*.md` in a path-table
is `glob = "docs/*.md"` in a config. A config table whose glob matches no files prints
`dirsql: table 'name': glob 'pattern' matched no files under <anchor>` to
stderr; the exit code is unchanged. Path-tables never warn.

## Edge cases

- A `{`, `}` or `[` with no partner is a literal character.
- A brace group with no comma and no `..` is literal (`{q}`); `{1..3}` and
  `{a..c}` expand, as do zero-padded
  `{01..10}` and stepped `{1..10..2}` ranges. Groups nest.
- `\[` is a literal bracket.
- POSIX classes (`[[:alpha:]]`) match Unicode characters, as bash does in a UTF-8
  locale: `[[:alpha:]]` matches `é` and `中`. `[[:digit:]]`, `[[:xdigit:]]` and
  `[[:ascii:]]` stay ASCII.
- A dot-named file or directory is listed only where a component spells the
  dot at that depth: `./**/.*` finds `docs/.draft.md` but does not enter
  `.config/`.
- A symlinked file is a file. A symlinked directory is entered by any component
  except `**`.
- A broken link lists nothing.
