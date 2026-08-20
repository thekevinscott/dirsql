# Configuration file (`.dirsql.toml`)

`.dirsql.toml` is a TOML file with one optional `[dirsql]` section, zero or
more `[[dirsql.extension]]` entries, zero or more `[[dirsql.function]]`
entries, and zero or more `[[table]]` entries.
An empty file is valid. A missing `[dirsql]` section behaves as an
all-defaults one. Unknown keys are a parse error at every level (top level,
`[dirsql]`, `[[table]]`, `[[dirsql.extension]]`, `[[dirsql.function]]`) — a
typo or a removed key fails loudly, naming the offending key, rather than
silently no-opping.

The [CLI](./cli.md) loads a config only when you pass it with `-c/--config`;
with none given [no named tables](./cli.md#configless-mode) are defined (a
`./.dirsql.toml` on disk is **not** auto-loaded). The [SDKs](./sdk.md) load a
config via the `config` constructor parameter.

**Path resolution.** Relative paths in the config (`[[dirsql.extension]]`
`path`) resolve against the config file's parent
directory. The **index root is not a config concern** — it is decided by the
runner (the CLI's invocation directory, or an SDK's explicit root), never by
the config file's location. See [`--config`](./cli.md#flags).

## `[dirsql]` keys

| Key | Type | Default | Description |
|---|---|---|---|
| `ignore` | array of strings | `[]` | Glob patterns matched against root-relative paths. Matched files are skipped entirely — excluded from the initial scan and from watch events. |

There is no timeout key. `on-file` hook runs are unbounded; to bound one, wrap
its command in `timeout(1)` (see [Command hooks](./hooks.md#bounding-a-hook)).
A config that still declares the removed `hook-timeout` key fails to load with
an error naming that replacement. [`[[dirsql.function]]`](#dirsql-function)
worker calls have their own per-call `timeout` key (default 30 seconds).

The top-level `.dirsql/` directory under the root is always excluded from
scanning, whether or not it appears in `ignore` — it is reserved for
`dirsql`'s own metadata (the persist cache lives there by default). Only the
top-level `.dirsql/` is reserved; a nested `sub/.dirsql/` is an ordinary
directory.

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]
```

Persistence is not a config key. Keep the SQLite index on disk between runs
with the [`--persist [PATH]` CLI flag](./cli.md#dirsql-server) — a machine-local
operational choice that belongs to the runner, not to shareable config.

## `[[dirsql.extension]]`

Each entry declares a SQLite extension to load at startup. Extensions are
loaded onto the connection before any `CREATE TABLE` runs; loading is
enabled only for the duration of each load and disabled again afterwards, so
the SQL `load_extension()` function is never exposed to queries.

| Key | Required | Description |
|---|---|---|
| `path` | yes (non-empty) | The extension's shared library. Either a **file path** (`.so` / `.dylib` / `.dll`; relative paths resolve against the config file's parent directory) or a bare **package name** (no path separator, no loadable-file suffix). The package name is runtime-specific — see [package-name resolution](#package-name-resolution) below. |
| `entrypoint` | no | Init-symbol override. When omitted, SQLite derives the entry point from the filename (`sqlite3_<filename>_init`); set this when that default does not match (e.g. `sqlite-vec`'s entry point is `sqlite3_vec_init`). |

```toml
[[dirsql.extension]]
path       = "./ext/vec0.dylib"
entrypoint = "sqlite3_vec_init"
```

### Package-name resolution

A `path` naming a package is resolved from the
installed package in the runtime environment: the Python launcher and SDK
use `importlib`, the Node launcher and SDK use `require.resolve` against
`node_modules`. Resolution is file-first (a same-named local file wins) and
errors when the package contains zero or multiple loadables for the current
platform. The **standalone Rust binary and the Rust SDK are
file-path-only** — they have no interpreter to resolve package names with.

The name must be what the runtime's resolver knows, which is not always the
name you installed:

- **Python** resolves the **importable module name** — underscores, not the
  pip distribution name. `pip install sqlite-vec` is loaded as
  `path = "sqlite_vec"`; `path = "sqlite-vec"` does not resolve.
- **Node** resolves the package whose install actually **contains the
  loadable**. Meta-packages that split binaries per platform resolve via the
  platform package — e.g. `npm install sqlite-vec` is loaded as
  `path = "sqlite-vec-linux-x64"` (matching your platform), not
  `path = "sqlite-vec"`, whose meta-package ships no loadable.

Extensions add **functions** callable in queries and in a table's DDL, and
**virtual tables** a table's [`ddl` batch](#batch-ddl) can create. What a
`[[table]]`'s own `name` may not be is a virtual table: that one is the
per-file row table `dirsql` inserts into. Create the virtual table alongside
it, under its own name.

## `[[dirsql.function]]`

Each entry declares a **worker-backed SQL scalar function**: a function
queries can call by name, whose values are computed by an external worker
process you (or a [plugin](../plugins.md)) provide. This is how a plugin adds
computed values — an embedding, a hash, a classification — to SQL without
`dirsql` knowing anything about the domain: the config names the function and
the command, the worker does the work. The first-party
[`dirsql-plugin-embeddings`](../plugins.md#dirsql-plugin-embeddings) declares
its `embed()` function exactly this way.

| Key | Required | Description |
|---|---|---|
| `name` | yes | The SQL name queries call. Must be a plain identifier — an ASCII letter or underscore followed by ASCII letters, digits, or underscores. |
| `args` | yes | The accepted arities (argument counts), each `0`–`127`. The function is registered once per listed arity, so `args = [1, 2]` makes both `f(x)` and `f(x, y)` callable and any other count a SQL error. An empty list, an out-of-range value, or a repeated value is a config error. |
| `command` | yes (non-empty) | The worker command. Argv-split with the same no-shell quoting rules as [command hooks](./hooks.md#argv-not-a-shell); runs in the config file's directory. |
| `deterministic` | no (default `false`) | When `true`, the function is registered with `SQLITE_DETERMINISTIC`, letting SQLite cache and reuse results for identical arguments within a query. Only set it when the worker really is a pure function of its arguments. |
| `timeout` | no | Per-**call** time bound: a positive integer is whole seconds (`timeout = 600`), a string is an integer suffixed `s` or `ms` (`"600s"`, `"250ms"`). When absent, the function mechanism's own 30-second default applies. |

```toml
[[dirsql.function]]
name          = "embed"
args          = [1, 2]
command       = "dirsql-plugin-embeddings worker"
deterministic = true
timeout       = "600s"   # generous: absorbs a first-call model download
```

### Worker lifecycle

Declaring a function is **inert**: at startup the function is registered on
the connection and nothing else happens. No process is spawned and nothing is
read until a query actually calls the function — a declared function nobody
calls costs nothing.

On the **first call**, `dirsql` spawns `command` and keeps that one worker
process alive for the rest of the invocation, sending it every subsequent
call — one process total, never one per row or per file. The worker is torn
down when the invocation ends.

The `timeout` bounds each **round-trip call**, not the query: a query that
calls the function on 10 000 rows is 10 000 individually timed calls. A call
that times out, or a worker that crashes or closes its pipes, fails the query
with an actionable error naming the function and command; the worker is
killed and the next call starts a fresh one.

Calling a function that no loaded config declares (say, the plugin providing
it is not installed) is SQLite's ordinary `no such function` error.

### Worker protocol

The worker speaks **newline-delimited JSON** over its stdin/stdout — one
request line in, one response line out, per call:

- **Request:** `{"call": [<arg>, ...]}` with the call's SQL arguments
  encoded as: TEXT → JSON string, INTEGER/REAL → JSON number, NULL → `null`,
  BLOB → `{"$bytes": "<base64>"}`.
- **Response:** `{"ok": <value>}` with the same scalar encodings — a JSON
  array or any other object is bound as TEXT, its JSON text (which is how an
  embedding worker returns a vector: `sqlite-vec`'s distance functions accept
  JSON-text vectors) — or `{"err": "message"}`, which **fails the query**
  with that message. An `{"err": ...}` response leaves the healthy worker
  running; only transport failures (timeout, crash) recycle it.
- **stderr passes through** to `dirsql`'s stderr, so a worker's progress
  bars and download logs reach the terminal.

## `[[table]]`

Each entry maps a glob pattern to a SQL table. A table's columns are exactly
what its required `on-file` command emits — dirsql injects nothing (see
[Columns](./columns.md)).

| Key | Required | Description |
|---|---|---|
| `name` | yes | The table's SQL name — the name you query it by. Declared, never derived from `ddl`: dirsql does not read the DDL text. The `ddl` must create a table by this name; if it doesn't, loading fails. |
| `ddl` | yes | A SQL batch, run verbatim — any number of statements. It must create a table called `name`; that table holds the file rows, and only the columns it declares are kept (keys the `on-file` command emits that are not declared are dropped). The rest of the batch is yours: indexes, virtual tables, triggers. See [Batch `ddl`](#batch-ddl). |
| `glob` | yes | Glob pattern matched against root-relative paths. Every table whose glob matches a file receives that file's rows — a file can populate multiple tables. A `{name}` segment is rewritten to `*` (it matches one path segment but captures nothing). |
| `on-file` | **yes** | A command run once per matched file; its stdout (a JSON array of row objects) becomes the file's rows. Must be non-empty. A `[[table]]` with no `on-file` is a load error (see [parse errors](#parse-errors)). See [Command hooks](./hooks.md#on-file). |
| `strict` | no (default `false`) | When `true`, rows whose keys do not exactly match the declared columns are rejected with an error: extra keys error, and every declared column must be supplied by the `on-file` output. When `false`, extra keys are dropped and missing columns become `NULL`. |

`on-file` is required because a table's rows come from nowhere else. dirsql
does not read file contents or merge filesystem facts on your behalf: the
command reads the file (it receives `{path}`) and prints the rows, and those
rows — filtered to the DDL — are the table. For plain stat columns with no
command, query the path directly with a [path-table](./path-tables.md)
instead of declaring a table.

```toml
[[table]]
name = "comments"
ddl     = "CREATE TABLE comments (path TEXT, author TEXT, body TEXT)"
glob    = "_comments/*/*.jsonl"
on-file = "jq -c -s '.' {path}"

[[table]]
name = "papers"
ddl     = "CREATE TABLE papers (paper_id TEXT, title TEXT)"
glob    = "**/meta.json"
on-file = "uv run python extract_papers.py {path}"
strict  = true
```

### Batch `ddl`

`ddl` is handed to SQLite whole, so a table declaration is not limited to one
statement:

```toml
[[table]]
name    = "messages"
glob    = "sessions/*/messages/*.json"
on-file = "jq -c '.' {path}"
ddl     = '''
CREATE TABLE messages (session TEXT, idx INT, role TEXT, text TEXT);
CREATE INDEX messages_session ON messages(session);

CREATE VIRTUAL TABLE messages_fts
  USING fts5(text, content='messages', content_rowid='rowid');
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
END;
'''
```

```bash
dirsql query "SELECT text FROM messages_fts WHERE messages_fts MATCH 'deploy'" \
  -c ./.dirsql.toml
```

Those two triggers are all a keyword index needs. dirsql writes file rows with
plain `INSERT` and `DELETE` — an update is a delete and an insert in one
transaction, and there is no `UPDATE` path on user rows — so triggers you
declare here stay current through the initial load and every
[watcher](../howto/react-to-changes.md) event. The same shape with a `vec0`
virtual table and an [`embed()`](#dirsql-function) call in the trigger gives
you stored vectors.

**dirsql never reads the batch.** After it runs, SQLite's own catalog
(`pragma_table_list`) settles what it produced:

- No table called `name` → a load error that lists what the batch *did*
  create, so a typo is obvious.
- `name` is a **virtual** table → a load error. The declared table is the one
  dirsql inserts file rows into, so it has to be a real row table. Create the
  virtual table alongside it, under its own name.
- `name` is **`WITHOUT ROWID`** → a warning. Internal row bookkeeping is keyed
  on rowid, so these will be rejected in a future release.

The whole batch runs in one transaction: if any statement fails, none of them
took effect, and the error is SQLite's own, prefixed with the config entry —
`table 'messages': SQLite error: near "(": syntax error`. Context, never
interpretation.

Two consequences of `ddl` running **once, when the table is created**:

- **Rows the batch inserts itself are not file-tracked.** No file owns them, so
  they survive file deletions — and they vanish on any rebuild.
- **Editing `ddl` at all rebuilds a
  [persistent cache](../howto/persist.md).** The config hash covers the entire
  batch, so a new index, a different FTS5
  tokenizer or a changed embedding model id forces a full sweep and re-ingest.
  That is the only invalidation lane: dirsql tracks no ownership of what the
  batch made.

### `on-file` row mapping

The command prints a JSON array of objects; each object becomes one row.
JSON values map to SQLite as: `null` → `NULL`; `true`/`false` → `1`/`0`; an
integral number → `INTEGER`, any other number → `REAL`; a string → `TEXT`; a
nested array or object → its JSON text as `TEXT`.

A row's columns are exactly the keys the command emits, narrowed to the DDL;
dirsql merges nothing else in. Output that is not a JSON array of objects is a
per-file failure: the file is skipped with a stderr warning and the scan
continues (see [failure semantics](./hooks.md#failure-semantics)).

## Composing multiple configs

Pass [`-c`/`--config`](./cli.md#flags) more than once to compose several config
files — a shared team config plus local overrides, or a plugin's config
alongside your own:

```bash
dirsql -c ./.dirsql.toml -c ~/team/embeddings.toml -c ./local.toml
```

The configs load and merge in **argv order**:

- **`[[table]]`, `ignore`, `[[dirsql.extension]]`, and `[[dirsql.function]]`
  entries accumulate** across all configs, in order.
- **Each config's `on-file` hooks and `[[dirsql.function]]` workers run from
  that config file's own directory** — so a relative command like
  `on-file = "sh ./extract.sh"` resolves against the config that declared it,
  wherever it lives.
- Each config is **validated on its own** (the [parse errors](#parse-errors)
  below apply per file). There is no cross-file merge validation, with two
  structural exceptions: **two configs defining a table of the same name is
  an error**, naming the table, and **two configs declaring a function of the
  same name is an error**, naming the function and both sources — never a
  silent last-writer-wins.

The index [root](./cli.md#flags) is the invocation directory regardless of where
any config lives. With no `-c`, [no named tables](./cli.md#configless-mode) are
defined (no `./.dirsql.toml` auto-discovery); a single `-c` behaves exactly as
before.

## Parse errors

Loading fails (the CLI enters [degraded mode](./cli.md#degraded-mode); the
SDKs raise/reject) when:

- The TOML is malformed.
- Any table contains an unknown key (top level, `[dirsql]`, `[[table]]`, or
  `[[dirsql.extension]]`). The error names the offending key.
- A `[[table]]` entry omits `name`, `ddl`, or `glob` (or `name` is
  empty/whitespace).
- A `[[table]]` entry's `ddl` runs but creates no table by its `name`. The
  error carries the entry's name, lists what the batch did create, and points
  at the fix:

  > `table 'messages': its `ddl` ran but created no table called 'messages' (it created: mesages, mesages_fts). Set `name` to the table the `ddl` creates.`

  dirsql asks SQLite's catalog rather than interpreting the DDL, so quoted
  (`CREATE TABLE "messages"`), schema-qualified (`main.messages`) and
  `IF NOT EXISTS` forms all match a plain `name = "messages"`.
- A `[[table]]` entry's `name` names a **virtual** table. The declared table
  holds the file rows, so it must be a real row table:

  > `table 'messages': its `ddl` created a virtual table called 'messages'. The declared table holds the file rows, so it must be a real row table; create the virtual table alongside it, under its own name.`

- A `[[table]]` entry's `ddl` is rejected by SQLite. Nothing the batch did
  takes effect, and SQLite's own message is passed through under the entry's
  name:

  > `table 'messages': SQLite error: near "(": syntax error`
- A `[[table]]` entry omits `on-file` (or it is empty/whitespace). The error
  names the offending glob and points at the fix:

  > `[[table]] '**/*.md' has no on-file hook, so every row would be all-NULL. Add an `on-file` hook that emits the columns, or, for stat columns with no code, query the path directly: `FROM './'``

- A `[[dirsql.extension]]` entry omits `path`, or `path` is empty.
- A `[[dirsql.function]]` entry omits `name`, `command`, or `args` (or
  `command` is empty/whitespace); its `name` is not a plain identifier; its
  `args` list is empty, repeats an arity, or lists one outside `0`–`127`; or
  its `timeout` is not positive whole seconds / a positive-integer `"...s"` or
  `"...ms"` string.
- `[dirsql]` declares the removed `hook-timeout` key (the error names the
  `timeout(1)` replacement).

## Full example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "dist/**"]

[[dirsql.extension]]
path       = "sqlite_vec"            # Python module name; on Node use the
                                     # platform package, e.g. sqlite-vec-linux-x64
entrypoint = "sqlite3_vec_init"

[[dirsql.function]]
name          = "embed"
args          = [1, 2]
command       = "dirsql-plugin-embeddings worker"
deterministic = true
timeout       = "600s"

[[table]]
name = "comments"
ddl     = "CREATE TABLE comments (author TEXT, body TEXT)"
glob    = "_comments/*/*.jsonl"
on-file = "jq -c -s '.' {path}"

[[table]]
name = "documents"
ddl     = "CREATE TABLE documents (title TEXT, summary TEXT)"
glob    = "**/index.md"
on-file = "uv run python extract_doc.py {path}"
```
