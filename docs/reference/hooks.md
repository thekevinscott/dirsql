# Command hooks

The `on-file` [config key](./config.md) (per `[[table]]`, also available as
the [`--on-file` flag](./cli.md#on-file-command) on `dirsql query`) runs an
external command under the execution contract below.

## Execution contract

### argv, not a shell

The command string is split into an argv with shell-like quoting: whitespace
separates arguments, and single or double quotes group them (so
`sh -c 'grep foo "$@" | sort' sh` keeps the quoted script as a single
argument). **No shell is invoked** — there is no globbing, piping, `$VAR`
expansion, or `&&`/`;` chaining. To get shell features, ask for a shell
explicitly with `sh -c '…'`.

A command that is empty, whitespace-only, or has unbalanced quotes is
invalid (empty/whitespace commands are already rejected at
[config parse time](./config.md#parse-errors)).

### Placeholders

A `{name}` in the command is substituted with its value, in every
occurrence, within whole argv tokens, in a single left-to-right pass:

- A substituted value is always exactly one argv element — a value
  containing spaces, quotes, or shell metacharacters stays a single
  argument. This makes untrusted input (file paths, request bodies)
  injection-safe at the argv level.
- Substituted values are never re-scanned: a value that itself contains
  `{…}` is inert.
- An unrecognized `{…}` is left literal.

The available placeholders are listed under the
[`on-file` contract](#on-file-contract) below.

### Working directory and environment

A command named by a `[[table]]` runs in the **config file's directory**, so
relative paths in the command resolve predictably regardless of where
`dirsql` was launched. A command named by the `--on-file` flag has no config
file: it runs in the **index root**, the directory `dirsql query` was run in,
which is also the directory a `./` path-table is relative to. In both forms
`{root}` is where the table's glob is anchored: the index root for `--on-file`,
the [glob anchor](./config.md#glob-anchor) for a `[[table]]`. The command inherits `dirsql`'s environment, so
tools like `uvx --with …` / `npx …` resolve their dependencies as usual.

### stdout protocol

The command's stdout is **NDJSON: one JSON object per line**, one row each.
Blank lines are skipped. A command that exits successfully and prints nothing
yields no rows. Every non-blank line must be a JSON object: a line that is
an array, a scalar, or log chatter fails the table, and the error names the
line number. There is no array form.

stderr is never data — it is captured only to enrich error messages (the
last 2 000 characters are attached to failures). Send logs there.

::: tip Print one object per line
`jq` users: pass `-c` so each object is emitted compactly on one line. Do not
slurp into an array (`jq -s`, `jq -n '[inputs]'`).
:::

### Bounding a hook

Hook runs are **unbounded** — `dirsql` imposes no timeout of its own. To
bound a hook, make the bound part of the command by wrapping it in
`timeout(1)`:

```toml
on-file = "timeout 30 my-extractor"
```

When the wrapper kills an overrunning command, the run exits non-zero and
the ordinary [failure semantics](#failure-semantics) apply — the table's
command failed, so the build fails.

::: warning Windows
Windows's built-in `timeout` command is a *sleep*, not a bound — it cannot
wrap another command. On Windows, bound the work inside the command itself
(or accept unbounded runs).
:::

[`[[dirsql.function]]`](./config.md#dirsql-function) worker calls are
different: a call is a round-trip on a persistent worker process, which
`timeout(1)` cannot express, so the function mechanism carries its own
per-call `timeout` key with a 30-second default.

### Failure semantics

A hook run fails when the command:

- cannot be spawned (e.g. the program is not found),
- exits non-zero (the exit code — or `signal`, if killed by one — and the
  stderr tail are reported; a `timeout(1)` wrapper killing an overrun lands
  here),
- or prints output where a non-blank line is not a JSON object (an array
  line is rejected with a message saying to print one object per line).

What a failure *means*: the command ran once for the whole table, so its
failure is the table's, and a table that cannot be filled fails the build.
The error names the table and carries the command's exit status and stderr
tail; the CLI prints it and exits `1`. A row the table rejects under `strict`
counts as the same kind of failure.

## `on-file` contract

Runs **once per table**, with every file matched by the table's `glob`
appended to the command as a trailing argument, and prints **one JSON object
per line** for the whole table, one row each. That is the whole contract. The command
reads the files itself; see [`[[table]]`](./config.md#table) for the
row-mapping rules.

One process over all the files is the shape to write: a loop over the
arguments that prints each row as it is parsed. Parsing is the
command's cost, not dirsql's — dirsql spawns the command, waits, and reads
its stdout. A table over no matched files spawns nothing.

```python
import json, sys

for path in sys.argv[1:]:
    for row in parse(path):
        print(json.dumps(row))
```

There is no row-to-file attribution: dirsql does not know which file a row
came from, and a row that needs the path carries it as a column the command
emitted. In watch mode a change to any file under the table's glob re-runs
the command over all of the table's files and replaces the table's rows.

The same command is attachable two ways, over the same contract: the
`on-file` config key on a `[[table]]`, and the
[`--on-file <command>`](./cli.md#on-file-command) flag on `dirsql query`,
which attaches it to every [path-table](./path-tables.md#parsing-rows-with-on-file)
in the query. The command string is identical between the two spellings — the
flag is the inline form, the config key the declared form (see
[Parse your files into columns](../howto/parse-files-into-columns.md)). In both
spellings the table's columns are exactly what the command emits, narrowed to
the DDL — `dirsql` injects no filesystem facts either way. A command that wants
the path or stat metadata emits it (it has the paths).

| Placeholder | Value |
|---|---|
| `{root}` | The directory the table's glob is anchored at (the index root for `--on-file`; the config file's directory, or the literal prefix of an absolute / `~/` glob, for a `[[table]]`). Derive an anchor-relative path with `relpath(path, {root})`. |

The matched files' **absolute** paths are not placeholders: they are appended
to the command as trailing arguments, after everything written in the
command, so `on-file = "extract.py"` receives them as `sys.argv[1:]` — one run
per table, self-sufficient from any working directory. A tool that needs a
path anywhere other than last gets a wrapper script. A command that spells
`{path}` is rejected at startup:

```
on-file command `python3 extract.py {path}` uses `{path}`, but an on-file command now runs once per table with every matched path appended as trailing arguments and prints one JSON object per line. Remove `{path}` and read the paths from the command's arguments.
```

### Argument-list limits

Every platform caps the bytes one process may receive as arguments. When a
table's paths outgrow that cap, dirsql splits them into the fewest
consecutive runs that fit, runs the command once per run, and concatenates
the rows it prints. Runs execute concurrently, one per CPU at most, like
`xargs -P`, so rows keep their order within a run but not across runs; SQL
never promised row order, so `ORDER BY` what you need. The command cannot tell: each run is an
ordinary invocation with a subset of the paths. A command must therefore not
assume one invocation sees every path — a count, a cross-file join, or a
dedupe over `sys.argv[1:]` is per run, not per table. Do that work in SQL.
