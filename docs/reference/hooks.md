# Command hooks

Three [config keys](./config.md) run an external command: `on-file` (per
`[[table]]`), and the server-wide `pre-query` and `post-query` (under
`[dirsql]`; CLI server only). All three share one execution contract.

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

Which placeholders exist depends on the hook (see
[per-hook contracts](#per-hook-contracts) below).

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

### Timeout

Every hook run is bounded by a **30-second** default timeout. A run
exceeding it is killed and treated as a failure. One global config key
raises (or tightens) the bound for **all** hooks:

```toml
[dirsql]
hook-timeout = 300   # positive whole seconds
```

Zero and negative values are a config error.

### Failure semantics

A hook run fails when the command:

- cannot be spawned (e.g. the program is not found),
- exits non-zero (the exit code — or `signal`, if killed by one — and the
  stderr tail are reported),
- exceeds the timeout (killed; stderr tail reported),
- exits zero but prints no non-empty stdout line,
- or (per hook, below) prints output that does not parse as expected.

What a failure *means* differs per hook:

| Hook | On failure |
|---|---|
| `on-file` | **Per-file isolation.** The file contributes no rows; a one-line warning naming the file and error goes to stderr; the scan continues. One bad file never aborts the scan. |
| `pre-query` | The request returns `500 Internal Server Error` with the command's stderr tail in the JSON `error` body. |
| `post-query` | The request returns `500 Internal Server Error` with the command's stderr tail (or, for unparseable output, `post-query did not return valid JSON: <err>`) in the JSON `error` body. |

## Per-hook contracts

### `on-file`

Runs once per file matched by the table's `glob`, at initial scan and on
every watched change. The command reads the file itself and prints a JSON
array of row objects; see [`[[table]]`](./config.md#table) for the
row-mapping rules.

| Placeholder | Value |
|---|---|
| `{path}` | The matched file's path **relative to the index root**. Appended automatically as a final argument when the command omits it, so `extract.py` and `extract.py {path}` behave identically. |
| `{root}` | The index root directory. |

### `pre-query`

Runs once per `POST /query` request, before the query. The raw request body
goes in; plain-text SQL comes out (the stdout payload line). `dirsql` runs
that SQL and returns rows as usual. With no `pre-query` key, the body is
parsed as `{"sql": …}` instead — see [HTTP API](./http-api.md#post-query).

| Placeholder | Value |
|---|---|
| `{args}` | The raw `POST /query` request body, verbatim, as one argv token. |

**The hook owns SQL safety.** The `{args}` substitution keeps the untrusted
body inert *as an argv token*, but whatever SQL string the hook prints is
executed as-is. Validate, escape, or parameterize inside the hook.

### `post-query`

Runs once per successful `POST /query`, after the query. The result rows
are serialized to a JSON array and delivered two ways:

- **On stdin** — always, unbounded. This is the recommended path.
- **As `{args}`** — only when the serialized payload is ≤ **96 KiB**. Above
  that, `{args}` is substituted with an **empty string** and a stderr
  warning naming the byte size directs the operator to stdin. The full
  payload is still on stdin — this is a fallback, not truncation.

| Placeholder | Value |
|---|---|
| `{args}` | The result rows as a JSON array, as one argv token; emptied (with a stderr warning) when the payload exceeds 96 KiB. |

The stdout payload line is parsed as JSON and returned verbatim as the
`200 application/json` response body. A payload that is not valid JSON
fails the request (`500`). With no `post-query` key, the bare row array is
returned — see [HTTP API](./http-api.md#post-query).
