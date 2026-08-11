# Command hooks

The `on-file` [config key](./config.md) (per `[[table]]`, also available as
the [`--on-file` flag](./cli.md#on-file-command) on `dirsql query`) runs an
external command under the execution contract below.

## Execution contract

### argv, not a shell

The command string is split into an argv with shell-like quoting: whitespace
separates arguments, and single or double quotes group them (so
`sh -c 'grep foo {path} | sort'` keeps the quoted script as a single
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

The command runs in the **config file's directory**, so relative paths in
the command resolve predictably regardless of where `dirsql` was launched.
It inherits `dirsql`'s environment, so tools like `uvx --with …` / `npx …`
resolve their dependencies as usual.

### stdout protocol

The command's result payload is the **last non-empty line of stdout**,
trimmed. Any log or chatter lines above it are ignored. A command that
exits successfully but prints no non-empty line is a failure ("produced no
output on stdout").

stderr is never data — it is captured only to enrich error messages (the
last 2 000 characters are attached to failures).

::: tip Print single-line output
Because only the last non-empty line is the payload, multi-line output loses
everything above the last line. `jq` users: pass `-c` so the JSON is emitted
compactly on one line.
:::

### Bounding a hook

Hook runs are **unbounded** — `dirsql` imposes no timeout of its own. To
bound a hook, make the bound part of the command by wrapping it in
`timeout(1)`:

```toml
on-file = "timeout 30 my-extractor {path}"
```

When the wrapper kills an overrunning command, the run exits non-zero and
the ordinary [failure semantics](#failure-semantics) apply — the file is
skipped, the scan continues.

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
- exits zero but prints no non-empty stdout line,
- or prints output that does not parse as a JSON array of row objects.

What a failure *means*: **per-file isolation.** The file contributes no rows
and is reported as skipped; the scan indexes every other file and commits.
The CLI names up to ten skipped files on stderr, then `... and N more`, and
exits `23` — distinct from `0` (clean) and `1` (the run failed), so a caller
can tell a partial index from a complete one. A row the table rejects under
`strict` counts as the same kind of failure.

## `on-file` contract

Runs once per file matched by the table's `glob`, at initial scan and on
every watched change. The command reads the file itself and prints a JSON
array of row objects; see [`[[table]]`](./config.md#table) for the
row-mapping rules.

The same command is attachable two ways, over the same contract: the
`on-file` config key on a `[[table]]`, and the
[`--on-file <command>`](./cli.md#on-file-command) flag on `dirsql query`,
which attaches it to every [path-table](./path-tables.md#parsing-rows-with-on-file)
in the query. The command string is identical between the two spellings — the
flag is the inline form, the config key the declared form (see
[Parse your files into columns](../howto/parse-files-into-columns.md)). In both
spellings the table's columns are exactly what the command emits, narrowed to
the DDL — `dirsql` injects no filesystem facts either way. A command that wants
the path or stat metadata emits it (it has `{path}`).

| Placeholder | Value |
|---|---|
| `{path}` | The matched file's **absolute** path. `on-file = "extract.py {path}"` — self-sufficient from any working directory, so the command resolves it even when the config lives outside the index. |
| `{root}` | The index root directory. Derive a root-relative path with `relpath({path}, {root})`. |
