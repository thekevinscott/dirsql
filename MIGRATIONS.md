# Migrations

Upgrade guides for `dirsql` consumers. Every release that breaks, removes, or
changes the runtime behavior of a public surface gets a migration entry.

**This file is frozen.** The entries below are the pre-fragment history. New
migration entries are **per-package fragments** under
`packages/<pkg>/migrations.d/` — one file per breaking change, each a complete
five-subsection entry — so PRs never conflict on a shared file. See each
package's `MIGRATIONS.md` (`packages/python`, `packages/ts`, `packages/rust`)
and AGENTS.md, "Changelog and Migrations". This frozen archive is still the
source of truth for the pre-fragment history; the docs site
([Migrations](https://thekevinscott.github.io/dirsql/migrations)) surfaces it
via a VitePress include — do not edit the rendered page.

See also: [`CHANGELOG.md`](https://github.com/thekevinscott/dirsql/blob/main/CHANGELOG.md) for the full release log. (The relative path is not used because this file is also included into the docs site via a VitePress include, where relative paths would break.)

## Frozen history (pre-fragment)

### The `[dirsql].root` config key is removed; the runner decides the index root (#540, epic #528)

#### Summary

`.dirsql.toml` no longer accepts a `root` key, and the index root is no longer
derived from the config file's location. A config file describes **content**
(tables, hooks); **where** you index is an operational fact owned by the
runner. One uniform rule now applies to every consumer (all three SDKs and the
CLI, which share the one Rust core): the index root is the **explicit root**
when given, else the **process cwd**. The config file's parent directory plays
no part. Combined with the strict parser (#536), a config that still carries
`root` fails loudly with `unknown field 'root'`.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` | `[dirsql]\nroot = "docs"` indexed `<config-dir>/docs` | remove the key; run `dirsql` from the directory you want indexed (or pass an explicit root via an SDK) |
| CLI, config elsewhere | `dirsql --config /elsewhere/.dirsql.toml` rooted at `/elsewhere` | roots at the invocation cwd; `cd` into the directory to index |
| Rust `DirSQL::from_config_path("/elsewhere/.dirsql.toml")` | rooted at `/elsewhere` | roots at the process cwd; add `.root("/elsewhere")` on the builder to keep the old target |
| Rust builder `.config(path)` without `.root()` | rooted at the config's parent | roots at the process cwd; add `.root(dir)` for an explicit target |
| Python `DirSQL(config="/elsewhere/.dirsql.toml")` | rooted at `/elsewhere` | roots at the process cwd; pass `root="/elsewhere"` to keep the old target |
| TypeScript `new DirSQL({ config: "/elsewhere/.dirsql.toml" })` | rooted at `/elsewhere` | roots at the process cwd; pass `{ root: "/elsewhere", config: … }` to keep the old target |

`DirSQL::from_config(dir)` (Rust) is unchanged: it still both reads
`dir/.dirsql.toml` and roots at `dir`. The default flow — `dirsql` with
`./.dirsql.toml`, or an SDK pointed at the cwd — is also unchanged (the config's
parent was already the cwd).

#### Deprecations removed

- The `[dirsql].root` config key. It is not a warning — an old config carrying
  it is a hard parse error (`unknown field 'root'`).

#### Behavior changes without code changes

- **A config carrying `root`** now fails to load: the CLI server degrades
  (`POST /query` → `503` whose `{"error": …}` names `root`; `dirsql query`
  exits non-zero), and the SDKs raise/reject.
- **`--config` / `from_config_path` / `config=` pointing at a config outside
  the cwd** now indexes the cwd, not the config's directory. Same code, new
  root.
- **The Rust builder with neither `.root()` nor `.config()`** no longer errors
  with "no root directory" — it roots at the process cwd. The
  explicit-root-vs-config-root collision warning on stderr is also gone.

#### Verification

```bash
mkdir -p /tmp/dirsql-540/data && cd /tmp/dirsql-540/data
printf '{}' > a.json
printf '[[table]]\nddl = "CREATE TABLE files (path TEXT)"\nglob = "*.json"\n' > /tmp/dirsql-540/.dirsql.toml
dirsql --config /tmp/dirsql-540/.dirsql.toml query "SELECT path FROM files"
# expected: [{"path":"a.json"}] — indexed from the cwd, not /tmp/dirsql-540

printf '[dirsql]\nroot = "docs"\n' > .dirsql.toml
dirsql query "SELECT 1"; echo "exit=$?"
# expected: non-zero exit; stderr names the unknown key `root`
```

### `persist` / `persist_path` removed from `.dirsql.toml`; use `--persist [PATH]` (#549, epic #528)

#### Summary

Persistence is no longer configured in `.dirsql.toml`. The `[dirsql].persist`
and `[dirsql].persist_path` keys are **removed** and replaced by a global
`--persist [PATH]` CLI flag. Whether and where to cache is a machine-local
operational fact — it belongs to the command you run, not to shareable config
content. This affects **CLI users** who enabled persistence via the config
file. Because unknown keys are now a hard error (#536), a config still carrying
either key fails to load rather than silently ignoring it: the server degrades
(`503` naming the key) and `dirsql query` exits non-zero. The SDK builder's
`persist` / `persist_path` constructor parameters are **unchanged** — only the
config-file keys and their builder-side wiring are gone.

#### Required changes

Delete the keys from `.dirsql.toml` and pass the flag instead.

| Surface | Before (`.dirsql.toml`) | After (CLI) |
| ------- | ----------------------- | ----------- |
| Default cache location | `[dirsql]`<br>`persist = true` | `dirsql --persist` |
| Custom cache location | `[dirsql]`<br>`persist = true`<br>`persist_path = "/var/cache/x.db"` | `dirsql --persist /var/cache/x.db` |
| One-shot query | `persist = true` in config | `dirsql query "SELECT …" --persist` (flag trailing so its optional value does not swallow the SQL) |

The default cache location is unchanged: bare `--persist` writes
`<root>/.dirsql/cache.db`, exactly where `persist = true` used to.

#### Deprecations removed

- `[dirsql].persist` and `[dirsql].persist_path` config keys. They are not
  soft-deprecated — they are removed and now parse-error as unknown fields.

#### Behavior changes without code changes

- **A config carrying `persist` / `persist_path`** previously enabled the
  on-disk cache; it now fails to load (unknown-field error). Move the intent to
  the `--persist` flag.
- **Cache/reconcile behavior is otherwise identical.** The on-disk format, the
  default path, and the stat-based trust/rebuild logic are unchanged — only the
  switch that turns persistence on moved from config to the CLI.

#### Verification

```bash
# The removed key is now a hard error:
printf '[dirsql]\npersist = true\n' > .dirsql.toml
dirsql query "SELECT 1"; echo "exit=$?"
# expected: non-zero exit; stderr names the unknown key `persist`

# The flag replaces it (run against a directory with no such config):
rm -f .dirsql.toml
dirsql query "SELECT COUNT(*) AS n FROM files" --persist
ls .dirsql/cache.db   # the cache was written at the default location
```

### Unknown `.dirsql.toml` keys are now a hard error (#536, epic #528)

#### Summary

`.dirsql.toml` parsing previously **ignored unknown keys** at every schema
level — a misspelled or removed key (`glbo` for `[dirsql]`, `persistpath` for
`persist_path`, a stale `format` on a `[[table]]`) silently no-opped, so a
config that looked applied did nothing. Every raw config struct now sets
`deny_unknown_fields`, so an unknown key at the top level, in `[dirsql]`, in a
`[[table]]`, or in a `[[dirsql.extension]]` is a parse error naming the key.
This affects **all consumers** that load a config (all three SDKs and the CLI),
since they share the one Rust parser. It was made to stop typos from silently
no-opping and to make future key removals fail loudly.

#### Required changes

Fix or remove any key the parser does not recognize. The error names it.

| Surface | Before | After |
| ------- | ------ | ----- |
| `[dirsql]` typo | `ignorre = ["*.tmp"]` (silently ignored) | `ignore = ["*.tmp"]` |
| `[[table]]` stale key | `format = "json"` (silently ignored) | remove the key |
| Any unknown key | loaded, key dropped | parse error naming the key (`unknown field 'ignorre', expected one of …`) |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Config load with an unknown key**: previously succeeded with the key
  dropped; now fails. On the CLI server this degrades the server — `POST
  /query` returns `503` whose `{"error": …}` body names the unknown key — and
  `dirsql query` exits non-zero with the same diagnostic on stderr. The SDKs
  raise/reject. A config that already parsed clean is unaffected.

#### Verification

```bash
printf '[dirsql]\npersistpath = "cache.db"\n' > .dirsql.toml
dirsql query "SELECT 1"; echo "exit=$?"
# expected: a non-zero exit; stderr names the unknown key `persistpath`
```

### `on-file` `{abspath}` token removed (#539, epic #528)

#### Summary

The per-file `on-file` command hook no longer recognizes the `{abspath}` token
(the matched file's absolute path). The token set is now `{path}` (relative to
the index root) and `{root}`. This affects **every SDK** (the behavior lives in
the shared Rust core, so `pip`/`npm`/`cargo` change together) and any
`.dirsql.toml` whose `on-file` command interpolated `{abspath}`. Unknown `{…}`
sequences are left literal by design (so shell/`jq` braces survive), so a stale
`{abspath}` does not raise — it is passed to the command as the **literal string
`{abspath}`**, which will typically fail per-file at runtime (e.g. no such
file). Update templates to use `{path}`.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` `on-file` using `{abspath}` | `on-file = "extract.py {abspath}"` | `on-file = "extract.py {path}"` |

#### Deprecations removed

- The `on-file` `{abspath}` placeholder — removed; use `{path}`.

#### Behavior changes without code changes

- **`on-file` templates referencing `{abspath}`**: previously substituted with
  the matched file's absolute path; now left literal (unknown token), so the
  command receives the string `{abspath}`. It no longer resolves to a path, so
  a command like `cat {abspath}` fails per-file (skipped with a stderr warning)
  and the file contributes no rows.

#### Verification

```bash
cat > .dirsql.toml <<'TOML'
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
on-file = "cat {path}"
TOML
printf '[{"name":"widget"}]' > a.json
dirsql query "SELECT name FROM items"
# expected: [{"name":"widget"}]
# (an `on-file = "cat {abspath}"` template now passes the literal "{abspath}"
#  to cat, which fails, so the row is absent and dirsql warns on stderr)
```

### `on-file` no longer appends `{path}` when the template omits it (#538, epic #528)

#### Summary

The `on-file` per-table command hook dropped its append-if-absent ergonomic:
token interpolation is now the only way the matched file's path reaches the
command. Previously `on-file = "cat"` implicitly appended the file's
root-relative path as a trailing argument (behaving like `cat {path}`); now a
template that never references `{path}` receives no path at all. This affects
**every SDK** (the behavior lives in the shared Rust core, so pip/npm/cargo all
change together) and any `.dirsql.toml` whose `on-file` command relied on the
implicit append. It was removed because a single explicit interpolation channel
is simpler to reason about and maintain than a substitute-or-append hybrid.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` `on-file` relying on implicit path | `on-file = "extract.py"` | `on-file = "extract.py {path}"` |
| Rust `dirsql::command::Placeholder` | `Placeholder::append(name, value)` | `Placeholder::new(name, value)` (substitute-only; no append variant) |

#### Deprecations removed

- `dirsql::command::Placeholder::append` and the `Placeholder::append_if_absent`
  field — removed; construct placeholders with `Placeholder::new`.

#### Behavior changes without code changes

- **`on-file` templates without `{path}`**: previously the matched file's
  relative path was appended as a trailing argv element, so `on-file = "cat"`
  read the file; now nothing is appended, so `cat` runs against its null stdin,
  produces no output, and the file is skipped per-file with a stderr warning
  (`on-file command failed: command 'cat' produced no output on stdout`) and
  contributes no rows. Add `{path}` to the template to restore the path.

#### Verification

```bash
cat > .dirsql.toml <<'TOML'
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
on-file = "cat {path}"
TOML
printf '[{"name":"widget"}]' > a.json
dirsql query "SELECT name FROM items"
# expected: [{"name":"widget"}]
# (with the {path}-less `on-file = "cat"`, the row is absent and dirsql warns on stderr)
```

### `on-file` `{path}` token now interpolates the absolute path (#542, part of #528)

#### Summary

The `on-file` command hook's `{path}` placeholder previously interpolated the
matched file's path **relative to the index root**; it now interpolates the
file's **absolute** path. This is a shared-core change (`packages/rust/src`)
compiled into all three installs (`cargo` / `pip` / `npm`), so every SDK and the
CLI behave identically. The motivation is #528's repeatable `--config`: a hook
runs with its working directory set to the declaring config file's directory, so
once a config can live outside the index a root-relative `{path}` no longer
resolves. An absolute path is self-sufficient from any cwd. Only the hook
**token** changed — the `path` **column** (stat virtuals) stays root-relative.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `on-file` command reading the file (`cat {path}`, `jq … {path}`) | received e.g. `books/a.json` | receives e.g. `/proj/books/a.json` — no change needed; the file still opens |
| `on-file` command that **concatenates** `{path}` onto a base (`sh -c 'cat {root}/{path}'`) | `{root}/books/a.json` resolved | now `{root}//proj/books/a.json` — drop the `{root}/` prefix and use `{path}` alone |
| `on-file` command **storing** `{path}` as a column value | stored the root-relative path | stores the absolute path — derive the relative form with `relpath({path}, {root})` if you need it |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **`on-file` `{path}` placeholder**: previously the file's path relative to the
  index root; now the file's absolute path. Commands that only read the file
  (`{path}` passed to `cat`/`jq`/an interpreter) are unaffected. Commands that
  prefix `{path}` with the root or persist it as data must adjust (see the table
  above). `{root}` is unchanged; the `path` column is unchanged (still
  root-relative).

#### Verification

```bash
mkdir -p /tmp/dirsql-542/data
cat > /tmp/dirsql-542/echo-path.sh <<'EOF'
#!/bin/sh
printf '[{"seen":"%s"}]' "$1"
EOF
cat > /tmp/dirsql-542/.dirsql.toml <<'EOF'
[[table]]
ddl = "CREATE TABLE f (seen TEXT)"
glob = "data/*.json"
on-file = "sh echo-path.sh {path}"
EOF
echo '[]' > /tmp/dirsql-542/data/a.json
cd /tmp/dirsql-542 && npx -y dirsql query "SELECT seen FROM f"
# expected: [{"seen":"/tmp/dirsql-542/data/a.json"}]  (absolute, not "data/a.json")
```

### Rust builder: `.persist(bool)` + `.persist_path()` collapse into one optional-path `.persist()` (#551)

#### Summary

The Rust `DirSQLBuilder` persist surface changed. The two methods
`.persist(persist: bool)` and `.persist_path(path)` are replaced by a single
`.persist(path: Option<impl AsRef<Path>>)`: `None` enables persistence at the
default `<root>/.dirsql/cache.db`, and `Some(path)` enables it at `path`.
Persistence is off unless `.persist(..)` is called. This affects **the Rust SDK
only** — the Python and TypeScript constructor parameters (`persist`,
`persist_path`/`persistPath`) are unchanged. Cache/reconcile behavior is
identical; this is purely a builder call-site change.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Rust builder, default cache path | `.persist(true)` | `.persist(None::<&Path>)` |
| Rust builder, custom cache path | `.persist(true).persist_path(&cache)` | `.persist(Some(&cache))` |
| Rust builder, persistence off | `.persist(false)` (or omit) | omit `.persist(..)` |

#### Deprecations removed

- `DirSQLBuilder::persist_path(path)` — removed; pass the path to
  `.persist(Some(path))` instead.

#### Behavior changes without code changes

_None._ The default path and the custom-path override behave exactly as
before.

#### Verification

```bash
cargo build -p dirsql
# expected: builds clean; a call site still using `.persist_path(..)` or
# `.persist(true)` fails to compile (E0599 no method `persist_path` /
# mismatched types for `persist`), confirming the collapse.
```

### Python wheels are now stable-ABI (abi3): wheel filename tag changes (#487, epic #480)

#### Summary

The `dirsql` PyPI package now ships **abi3 (stable-ABI) wheels**: the pyo3
binding enables the `abi3-py311` feature, so maturin builds one
`cp311-abi3` wheel per platform (loadable on every CPython ≥ 3.11) instead of a
separate `cp311`/`cp312`/`cp313`/`cp314` wheel per interpreter. This affects the
**Python SDK's published artifacts only** — no import surface, call site, or
supported-version floor changes (`requires-python` stays `>=3.11`). The change
cuts the release build matrix' per-Python-version wheel axis 4×. The only
consumer-visible difference is the wheel **filename tag**.

#### Required changes

_None._ `pip install dirsql`, `uv add dirsql`, and `uvx dirsql` resolve and
install the abi3 wheel unchanged on CPython 3.11 through 3.14.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Wheel filename tag**: previously the interpreter-specific
  `dirsql-<ver>-cp312-cp312-<platform>.whl` (one per Python minor version); now
  the stable-ABI `dirsql-<ver>-cp311-abi3-<platform>.whl` (one per platform,
  installed on 3.11+). Consumers that **pin or hash exact wheel filenames** (a
  lockfile with per-file hashes, an internal mirror keyed by filename, or CI
  that greps the tag) must refresh those references. Everyone installing by
  package name and version is unaffected.

#### Verification

```bash
pip download dirsql --no-deps --python-version 3.13 --only-binary=:all: -d /tmp/dirsql-whl
ls /tmp/dirsql-whl
# expected: a single dirsql-<ver>-cp311-abi3-<platform>.whl (no cp313-cp313 wheel)
python3.13 -c "import dirsql; print('ok')"
# expected: ok
```

### Binding-boundary value fidelity: out-of-range integers error, list-of-ints is no longer bytes, extract errors carry the real message (#465, epic #461)

#### Summary

The Python and TypeScript bindings marshaled some `extract` values and query
results incorrectly. A Python `int` larger than `i64` silently degraded to a
lossy `REAL` (via `__float__`) or a `TEXT` repr; a query result larger than
JavaScript's safe integer range silently rounded to the nearest `number`; a
JS `bigint` was stored as `TEXT`; a Python `list`/`tuple` of ints in the
0–255 range was probed as bytes and stored as a `BLOB` (so `[1,2,3]` became
a BLOB but `[1,2,300]` became `TEXT` — "bytes by magnitude"); and a JS
`extract` that threw surfaced only the fixed string
`"Extract function call failed"`, discarding the real message. All are fixed
with a single symmetric numeric contract: integers that do not fit a signed
64-bit `Value::Integer` raise/throw an explicit range error, and only genuine
binary types map to `BLOB`. This changes runtime behavior at the SDK
boundary; the public API signatures are unchanged.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python `extract` returning an `int` > `i64` | silently stored as lossy `REAL`/`TEXT` | raises `OverflowError` (surfaced as `RuntimeError` from `ready()`) — pass a `str` or a fitting `int` |
| TypeScript query result > `Number.MAX_SAFE_INTEGER` | returned a rounded `number` | `query()` rejects — store such values as `TEXT` if you must read them as JS numbers |
| TypeScript `extract` returning a `bigint` in `i64` range | stored as `TEXT` (`"42"`) | stored as `INTEGER` (`42`) |
| TypeScript `extract` returning a `bigint` outside `i64` | stored as `TEXT` | throws — pass a `string` instead |
| Python `extract` returning `[1, 2, 3]` | stored as `BLOB` `b"\x01\x02\x03"` | stored as `TEXT` `"[1, 2, 3]"` — pass `bytes(...)` for a BLOB |
| napi `extract` that throws | `"Extract function call failed"` | the thrown `Error`'s real `message` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

All of the above are behavior changes with no API-signature change: the same
`extract` callbacks and `query()` calls now raise/throw (or return a
differently-typed value) for the inputs listed. Code that already passed
in-range integers, real `bytes`/`Buffer` values, and non-throwing extracts is
unaffected.

#### Verification

```python
# Python: an out-of-i64 int now raises rather than corrupting.
import asyncio, tempfile, os
from dirsql import DirSQL, Table
d = tempfile.mkdtemp(); open(os.path.join(d, "m.json"), "w").write("{}")
db = DirSQL(d, tables=[Table(ddl="CREATE TABLE t (v)", glob="*.json",
                             extract=lambda p: [{"v": 2**63}])])
try:
    asyncio.run(db.ready())
except RuntimeError as e:
    print("raised:", "exceeds" in str(e))   # -> raised: True
```

### `query()` now rejects `ATTACH`/`DETACH` (#462, epic #461)

#### Summary

`query()` — the read-only SQL surface exposed by every SDK (`DirSQL.query`) and
by the CLI (`POST /query`, `dirsql query "<sql>"`) — previously allowed
`ATTACH` and `DETACH`. SQLite reports `ATTACH` as read-only via
`sqlite3_stmt_readonly`, so it passed the read-only gate; but `ATTACH` creates a
file on disk and opens an arbitrary external database, which a follow-up
`SELECT ... FROM ext.*` could then read. The query-path authorizer now denies
both statements at prepare time. This is a behavior change with no API change:
the same method signatures accept the same inputs, but an `ATTACH`/`DETACH`
that used to succeed now raises a not-authorized error.

#### Required changes

_None._ The method signatures are unchanged. Callers that relied on
`ATTACH`/`DETACH` through `query()` (never a documented capability) must stop;
there is no replacement on this surface.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `DirSQL.query` / `POST /query` / `dirsql query`: an `ATTACH` or `DETACH`
  statement previously prepared and executed (creating/opening the target
  database); it now fails at prepare time with a not-authorized error
  (`DirSqlError::Unauthorized` in Rust; a raised exception carrying
  `not authorized` in Python/TypeScript) and no file is created. All other
  effectful statements already failed the read-only gate and are unaffected.

#### Verification

```bash
dirsql query "ATTACH '/tmp/evil.db' AS ext"
# expected: a non-zero exit with an error mentioning "not authorized",
# and /tmp/evil.db is NOT created.
```

### Watch now skips directory events and deletes rows on rename-out (#466, epic #461)

#### Summary

Two live-watch behaviors in the Rust core changed (no API change), affecting
every SDK and the CLI since they share the core watcher. (1) Creating a
directory under the watched root (`mkdir subdir`) previously inserted a
spurious row when a table's glob matched it (e.g. the default `files` table's
`**/*`); the watch upsert now re-checks that the path is a regular file and
skips non-files, mirroring the initial scan. (2) Renaming a matching file
*out* of the watched tree previously left its rows in the index (the
rename-away event was treated as a modification, then the now-missing file was
silently skipped); rename-out is now treated as a removal and its rows are
deleted. A third fix corrects `parse_table_name` DDL parsing (a leading
comment no longer hijacks the table name; Unicode in a comment no longer risks
a panic) — a pure bug fix with no observable behavior change for well-formed
DDL.

#### Required changes

_None._ No public API, config key, CLI flag, action input, function signature,
or return type changed.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **`mkdir` under the watched root no longer emits an `Insert`/adds a row.** A
  consumer that (incorrectly) relied on directories appearing as rows will no
  longer see them. Regular files are unaffected.
- **Renaming a matching file out of the watched tree now emits a `Delete` and
  removes its rows.** Previously those rows persisted until a restart. This is
  platform-dependent on the OS rename signal (verified on Linux `inotify`).

#### Verification

```bash
# In a watched directory with a `**/*`-matching table:
mkdir subdir            # no new row appears for `subdir`
mv a.txt ../a.txt       # the rows for a.txt disappear from the index
```

### The seven stat columns dropped their leading underscore (#454, epic #452)

#### Summary

The seven filesystem-fact columns dirsql auto-populates on every table row
were named `_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`.
The leading underscore suggested these were a protected/reserved namespace,
but nothing in the engine ever enforced that — a column declared with one of
these names was populated the same way regardless, with no validation
preventing a user from reusing the name for something else. The only
genuinely enforced reserved namespace in dirsql is `_dirsql_*` (denied at
prepare time by a SQLite authorizer), which is unaffected by this change.
The underscore prefix is dropped: the seven columns are now `path`,
`basename`, `dir`, `ext`, `size`, `mtime`, `ctime`. This is a pure rename —
the columns' values, types, population rules (opt-in by DDL), and
precedence versus a row source's own output are all unchanged.

This is a hard cutover with no backward-compatibility shim: a `.dirsql.toml`
or SDK table declaring the old names now simply gets a table with no
`path`/`basename`/etc. columns populated (the old names are no longer
recognized facts), not an error. Any config or code referencing the old
names must be updated.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` `[[table]]` `ddl` | `ddl = "CREATE TABLE files (_path TEXT, _size INTEGER)"` | `ddl = "CREATE TABLE files (path TEXT, size INTEGER)"` |
| SQL queries | `SELECT _path, _size FROM files` | `SELECT path, size FROM files` |
| Python `Table(ddl=...)` | `ddl="CREATE TABLE files (_path TEXT)"` | `ddl="CREATE TABLE files (path TEXT)"` |
| TypeScript `{ ddl, glob }` / `new Table({...})` | `ddl: "CREATE TABLE files (_path TEXT)"` | `ddl: "CREATE TABLE files (path TEXT)"` |
| Rust `Table::new(ddl, glob, extract)` | `"CREATE TABLE files (_path TEXT)"` | `"CREATE TABLE files (path TEXT)"` |
| Result-row field access (any SDK) | `row["_path"]` / `row._path` | `row["path"]` / `row.path` |

The same rename applies to every occurrence of the other five columns
(`_basename`/`_dir`/`_ext`/`_mtime`/`_ctime`) in each surface above.

#### Deprecations removed

_None._ There was no deprecation period for this change — the old names are
removed outright, not soft-deprecated first.

#### Behavior changes without code changes

_None beyond the rename itself._ A config or query that is not updated does
not error; it simply stops receiving the affected column's data (the
column, if declared under the old name, is never populated — same as
declaring any other column name dirsql doesn't recognize as a fact).

#### Verification

```bash
mkdir -p /tmp/dirsql-rename-demo && cd /tmp/dirsql-rename-demo
echo hi > notes.txt
cat > .dirsql.toml <<'EOF'
[[table]]
ddl  = "CREATE TABLE files (path TEXT, basename TEXT, size INTEGER)"
glob = "*.txt"
EOF
dirsql query "SELECT path, basename, size FROM files"
# expected: [{"basename":"notes.txt","path":"notes.txt","size":3}]
```

### `dirsql init` no longer runs `claude`; it writes a fixed starter config (#455, epic #452)

#### Summary

`dirsql init` previously shelled out to the `claude` CLI, prompting it to
inspect the target directory and generate a `.dirsql.toml`. That made `init`
non-deterministic, required a signed-in `claude` on `PATH`, and could drift
from the default `files` table zero-config mode actually serves. `init` now
writes a fixed starter config verbatim — the same single `files` table
zero-config mode parses from one embedded default-config asset shared by
both code paths, so the two can never disagree. `init` no longer inspects
the target directory's contents at all; `--root` now only controls where
the default `--output` path (`<root>/.dirsql.toml`) is resolved, not what
gets written. No LLM call, no network access, no `claude` dependency.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Running `dirsql init` | Requires `claude` on `PATH`, signed in; output varies by directory contents and LLM response. | No prerequisites beyond the `dirsql` binary; output is always the fixed starter config. |
| `--root <path>` | Directory `claude` scanned to generate content. | Directory the default `--output` path is resolved against only. |

No config file syntax changed — a `.dirsql.toml` previously generated by
`init` remains valid and loadable.

#### Deprecations removed

_None._

#### Behavior changes without code changes

`dirsql init` run against any directory now always produces byte-identical
output (the fixed starter config), regardless of that directory's contents.
Scripts or tests that asserted on directory-dependent generated content will
need to assert on the fixed content instead.

#### Verification

```bash
mkdir -p /tmp/dirsql-init-demo && cd /tmp/dirsql-init-demo
dirsql init
cat .dirsql.toml
# expected:
# [[table]]
# ddl  = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER, ctime INTEGER)"
# glob = "**/*"
dirsql query "SELECT path FROM files"
# expected: [{"path":".dirsql.toml"}]
```

### A rejected write statement via `query()` now returns HTTP 400, not 500 (#444)

#### Summary

`POST /query` (and the `dirsql query` subcommand) previously returned HTTP 500
for a write statement (e.g. `DELETE FROM files`), even though
`docs/reference/http-api.md` documents the read-only rule as a 400-class
error. The internal classification treated the read-only rejection
(`DirSqlError::WriteForbidden`) as a server-side fault instead of a caller
error. It now maps to 400, consistent with other caller-fixable SQL errors
(malformed SQL, unknown table) and the existing `_dirsql_*` internal-table
denial (#378).

#### Required changes

_None._ No API, config key, CLI flag, or return type changed.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **A write statement submitted to `query()` (via `POST /query` or
  `dirsql query`) now returns HTTP 400 (or, for the CLI, the same
  non-zero-exit/stderr treatment as any other bad-request error) instead of
  500.** Callers that specifically branched on 500 for this case should treat
  it as a 400 instead. The error message is unchanged.

#### Verification

```bash
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://localhost:<port>/query \
  -H 'content-type: application/json' \
  -d '{"sql": "DELETE FROM files"}'
# 400
```

### Default (non-persist) index moved from `:memory:` to an anonymous disk-backed temp database (#402)

#### Summary

With `persist = false` (the default), the SQLite index used to live entirely in
RAM (`:memory:`), so resident memory scaled with the indexed corpus and large
directories could OOM the host process. The engine now opens an **anonymous
SQLite temp database** instead: SQLite creates a private temp file, deletes it
immediately (the OS reclaims it even on SIGKILL), and spills index pages to
disk as the index grows — only the page cache stays resident. All SDKs and the
CLI are affected identically (shared core). The API is unchanged; queries,
watch events, and persistence semantics are identical.

#### Required changes

_None._ No API, config key, CLI flag, or return type changed.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Index pages now live in the system temp directory instead of RAM.** SQLite
  picks the directory via `SQLITE_TMPDIR` → `TMPDIR` → `/var/tmp` → `/usr/tmp`
  → `/tmp`. On hosts where the chosen directory is a tmpfs (RAM-backed) mount,
  export `SQLITE_TMPDIR` to point at a real disk to get the memory benefit.
- **Resident memory for large corpora drops** from O(indexed data) to roughly
  the SQLite page cache; disk usage in the temp directory grows correspondingly
  while the process runs (the space is reclaimed on exit, including crashes).
- Query latency on very large indexes is now bounded by the page cache rather
  than all-in-RAM access; for typical corpora the difference is negligible.

#### Verification

Run any dirsql process with `persist = false` over a directory and inspect its
open file descriptors while it runs (Linux):

```bash
ls -l /proc/<pid>/fd | grep etilqs
# lrwx------ ... 13 -> /var/tmp/etilqs_abc123 (deleted)
```

An `etilqs_*` entry marked `(deleted)` in your temp directory is the anonymous
index file. Before this change no such descriptor existed and the same data sat
in anonymous heap memory instead.

### Internal `_dirsql_*` bookkeeping tables are unreachable through `query()` (#378)

#### Summary

dirsql's internal bookkeeping tables — `_dirsql_internal_rows`, `_dirsql_files`,
`_dirsql_meta` — are no longer readable through the public `query()` surface. A
SQLite authorizer on the query path denies any read (or schema `PRAGMA`)
targeting the reserved `_dirsql_*` namespace, so such a query now errors at
prepare time ("not authorized") instead of returning the internal rows. This is
an engine-enforced encapsulation of an always-internal, always-undocumented
surface; documented usage is unaffected.

#### Required changes

_None for documented usage._ If you were relying on reading dirsql's internal
tables directly (e.g. `SELECT * FROM _dirsql_internal_rows` for row-ownership
bookkeeping), there is no supported replacement — that data is internal by
design. Query your own user tables instead.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `SELECT` / `PRAGMA` reading a `_dirsql_*` table through `query()` (SDK) or
  the CLI's `POST /query` now fails: an SDK `query()` returns a "not authorized"
  error, and `POST /query` responds `400` with a `not authorized` message,
  where both previously returned the internal rows.
- Normal user queries, and the engine's own internal read/write paths (indexing,
  delete-by-file, persistence), are unchanged.

#### Verification

```sh
dirsql query "SELECT * FROM _dirsql_internal_rows"; echo "exit: $?"
# expected: nonzero exit with a rejected-read diagnostic on stderr
# (was exit 0 with the internal rows before this change)
```

### Internal `_dirsql_*` tracking columns removed; one-time cache rebuild (#361)

#### Summary

dirsql no longer injects internal `_dirsql_file_path` / `_dirsql_row_index`
tracking columns into user tables; row ownership now lives entirely in the
internal `_dirsql_internal_rows` table. This is an internal change with no
effect on documented usage — `SELECT *` never returned these columns. The one
observable consequence is on-disk: the persistent-cache schema version is bumped
(`2` → `3`), so the first startup after upgrading discards and rebuilds the
cache once, automatically.

#### Required changes

_None for documented usage._ If you explicitly selected the (undocumented)
`_dirsql_file_path` / `_dirsql_row_index` columns, switch to the documented
filesystem-fact columns (`_path`, `_basename`, `_dir`, `_ext`, `_size`,
`_mtime`, `_ctime`); the `_dirsql_*` columns no longer exist and naming them
now errors with "no such column".

#### Deprecations removed

_None._

#### Behavior changes without code changes

- The persistent-cache schema version is bumped (`2` → `3`). A cache written by
  an older build is discarded and rebuilt once, automatically, on first startup
  — no action required, and penalty-free per the persistence design.
- `PRAGMA table_info(<table>)` and `SELECT *` now report exactly the user's
  declared columns. `SELECT *` results are unchanged (it already excluded the
  internal columns); the difference is that the columns no longer exist at all.

#### Verification

After upgrading, the first run rebuilds the cache and a table's schema is
verbatim. With a `.dirsql.toml` declaring
`[[table]] ddl = "CREATE TABLE files (_path TEXT)"`:

```sh
dirsql query "SELECT name FROM pragma_table_info('files')"
# expected: only the columns you declared (e.g. `_path`) — no _dirsql_file_path / _dirsql_row_index
```

### TypeScript: BLOB columns return `Buffer` instead of hex strings (#343)

#### Summary

The TypeScript SDK now round-trips binary values as real SQLite BLOBs,
restoring parity with Python's documented `bytes → BLOB` mapping. Two runtime
behaviors change for TypeScript consumers only: a `Buffer`/`Uint8Array`
returned from an `extract` callback is stored as a BLOB (previously it was
silently coerced to its string representation, e.g. `"0,1,2"`), and BLOB
columns come back from `query()` and watcher `RowEvent` rows as Node
`Buffer`s (previously lowercase hex strings). The API surface (signatures,
types) is unchanged — row values were already typed `unknown`. The CLI's
HTTP/JSON responses are unaffected (JSON cannot carry binary; blobs stay
hex-encoded there). Python and Rust are unaffected.

#### Required changes

_None_ for code that treated binary correctly end-to-end. Code that depended
on the old lossy representations must drop the workaround:

| Surface | Before | After |
| ------- | ------ | ----- |
| TS `query()` / `RowEvent` BLOB value | `const hex = row.data as string` | `const buf = row.data as Buffer` (render hex explicitly if needed: `buf.toString("hex")`) |
| TS `extract` returning binary | pre-encode to a string (e.g. `data.toString("hex")`) to control what was stored | return the `Buffer`/`Uint8Array` directly; it is stored as a BLOB |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- TS `extract` values: previously a `Buffer`/`Uint8Array` was coerced to its
  string representation and stored as TEXT; now it is stored as a BLOB.
- TS `query()` results and `RowEvent.row` / `RowEvent.oldRow`: previously a
  BLOB column surfaced as a lowercase hex string; now it surfaces as a
  `Buffer`. Code that compared against hex strings must compare `Buffer`s
  (or call `.toString("hex")`).

#### Verification

```bash
cd packages/ts && pnpm build
node --input-type=module -e '
import { DirSQL } from "./dist/index.js";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
const dir = mkdtempSync(join(tmpdir(), "dirsql-blob-"));
writeFileSync(join(dir, "a.json"), "{}");
const db = new DirSQL({ root: dir, tables: [{
  ddl: "CREATE TABLE blobs (data BLOB)",
  glob: "*.json",
  extract: () => [{ data: Buffer.from([0, 1, 2, 255]) }],
}]});
const rows = await db.query("SELECT * FROM blobs");
console.log(Buffer.isBuffer(rows[0].data), rows[0].data);
'
# expected: true <Buffer 00 01 02 ff>
```

### Python: native-language (`.py`) configs and `dirsql interpret` removed; serialization snapshot retired (#323)

#### Summary

The Python SDK's native-language config path and the `dirsql interpret`
subcommand are **hard-removed** (A1 of epic #321), with no deprecation window.
`dirsql --config <file>.py` is no longer supported, and `dirsql interpret …`
is no longer a subcommand (the Python launcher forwards it to the binary, which
rejects it). The Python side of the cross-language config-serialization
snapshot (#194) is retired with it: `DirSQL.__dict__` / `vars(db)` and the
`resolve_config` helper are gone. The **programmatic SDK** — `DirSQL(...)` with
in-process `Table(extract=fn)` closures — is unaffected, and
`DirSQL(config="…toml")` still loads TOML. Affects anyone running a Python
`.py` config via the CLI, invoking `dirsql interpret`, or reading `vars(db)` /
`db.__dict__`. (TypeScript is #324; Rust + docs #325.)

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| CLI Python config | `dirsql --config dirsql.config.py` | Not supported. Use a `.dirsql.toml`, or embed the SDK programmatically (`DirSQL(...)` + `Table(extract=fn)`) and query it in-process. |
| `dirsql interpret <config>` | long-running NDJSON helper subcommand | Removed; exits non-zero (unknown subcommand). |
| Python serialized state | `vars(db)` / `db.__dict__` → resolved-config dict | Removed. Pass `config=` / `root=` / `tables=` into the core and query. |

#### Deprecations removed

_None._ Native configs, `interpret`, and the serialization snapshot were never
deprecated; removed in a single release (the feature never shipped a stable
release).

#### Behavior changes without code changes

- `dirsql --config <file>.py` no longer spawns an interpreter. Once the Rust
  side lands (#325) the non-TOML file fails to parse as TOML and the server
  starts degraded (HTTP 503); until then the binary still spawns `dirsql
  interpret`, which the Python launcher no longer handles, so the helper exits
  non-zero and the server reports the spawn failure.
- `dirsql interpret …` (invoked directly) exits non-zero instead of starting an
  NDJSON helper.

#### Verification

```bash
cd packages/python
uv run dirsql interpret whatever.py; echo "exit=$?"
# expected: non-zero exit ("unrecognized subcommand 'interpret'")
uv run python -c "from dirsql.cli.interpret import run"; echo "exit=$?"
# expected: non-zero exit (ImportError — the interpret loop is gone; only an
# empty package shell remains, pending the tooling fix that lets the directory
# be deleted outright)
```

### TypeScript: native-language (`.js`/`.mjs`/`.cjs`) configs and `interpret` removed; serialization snapshot retired (#324)

#### Summary

The TypeScript SDK's native-language config path and the `interpret` CLI
dispatch are **hard-removed** (A2 of epic #321), with no deprecation window.
`dirsql --config <file>.{js,mjs,cjs}` is no longer supported, and `dirsql
interpret …` is no longer dispatched (the launcher forwards it to the binary,
which rejects it). The TypeScript side of the cross-language
config-serialization snapshot (#194) is retired with it: `DirSQL.toJSON()` /
`JSON.stringify(db)` and the `resolveConfig` helper are gone, along with the
now-unused `DirSQLConfig` / `TableConfig` / `ResolvedExtension` exported types.
The **programmatic SDK** — `new DirSQL(...)` with in-process `extract` closures
— is unaffected, and `new DirSQL("…toml")` still loads TOML. Affects anyone
running a `.js`/`.mjs`/`.cjs` config via the CLI, invoking `dirsql interpret`,
or calling `db.toJSON()` / `JSON.stringify(db)` / importing those types. (Rust
+ docs is #325.)

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| CLI JS config | `dirsql --config dirsql.config.mjs` (or `.js`/`.cjs`) | Not supported. Use a `.dirsql.toml`, or embed the SDK programmatically (`new DirSQL(...)` + `extract` closures) and query it in-process. |
| `dirsql interpret <config>` | long-running NDJSON helper subcommand | Removed; exits non-zero (unknown subcommand). |
| TypeScript serialized state | `db.toJSON()` / `JSON.stringify(db)` → `DirSQLConfig` | Removed, along with the `DirSQLConfig` / `TableConfig` / `ResolvedExtension` exported types. Pass `config` / `root` / `tables` into the constructor and query. |

#### Deprecations removed

_None._ Native configs, `interpret`, and the serialization snapshot were never
deprecated; removed in a single release (the feature never shipped a stable
release).

#### Behavior changes without code changes

- `dirsql --config <file>.{js,mjs,cjs}` no longer spawns an interpreter. Once
  the Rust side lands (#325) the non-TOML file fails to parse as TOML and the
  server starts degraded (HTTP 503); until then the binary still spawns `dirsql
  interpret`, which the launcher no longer handles, so the helper exits
  non-zero and the server reports the spawn failure.
- `dirsql interpret …` (invoked directly) exits non-zero instead of starting an
  NDJSON helper.

#### Verification

```bash
cd packages/ts
pnpm build
node dist/cli/dirsql.js interpret whatever.mjs; echo "exit=$?"
# expected: non-zero exit ("unrecognized subcommand 'interpret'")
node --input-type=module -e "import { DirSQL } from './dist/index.js'; const db = new DirSQL({ root: '.' }); console.log(typeof db.toJSON); db.ready.catch(() => {});"
# expected: undefined (toJSON was removed)
```

### Rust: native-config orchestration and `DirSQL::config()` / `DirSQLConfig` removed; `.dirsql.toml` is the only CLI config (#325)

#### Summary

The Rust core and CLI binary drop the native-language config path (A3 of epic
#321, lands last), completing the epic across all three SDKs. The `dirsql`
binary no longer inspects the `--config` extension or spawns a `dirsql
interpret` helper, and `cli::native_config` (the NDJSON spawn/handshake
protocol) is deleted. `.dirsql.toml` is the only config format the CLI accepts
(unchanged for TOML users). The cross-language config-serialization snapshot
(#194) is retired: `DirSQL::config()` and the `DirSQLConfig` type are removed
from the Rust SDK — nothing consumed the serialized state once `interpret` was
gone. Affects anyone passing a `.py`/`.js`/`.mjs`/`.cjs` file to `dirsql
--config`, and any Rust caller of `db.config()` / `DirSQLConfig`.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| CLI native-language config | `dirsql --config config.py` (or `.js`/`.mjs`/`.cjs`) — binary spawned `dirsql interpret` | Not supported. The non-TOML file fails to parse as TOML; the server starts degraded (HTTP 503). Use a `.dirsql.toml`, or embed a binding SDK programmatically. |
| Rust serialized state | `db.config() -> DirSQLConfig` (`serde::Serialize`) | Removed, along with the `DirSQLConfig` / snapshot `TableConfig` types. `DirSQLBuilder::config(path)` (loads a `.dirsql.toml`) and `DirSQL::from_config_path` are unchanged. |

#### Deprecations removed

_None._ Native configs, `interpret`, and the serialization snapshot were never
deprecated; removed in a single release (the feature never shipped a stable
release).

#### Behavior changes without code changes

- `dirsql --config <file>.{py,js,mjs,cjs}` no longer spawns a `dirsql
  interpret` helper. The binary treats every `--config` as TOML, so a
  non-TOML file fails to parse and the server starts in the degraded
  (HTTP 503) state with the parse diagnostic, instead of serving the config's
  tables.
- `dirsql interpret …` (any SDK's launcher) exits non-zero — the subcommand no
  longer exists.

#### Verification

```bash
cargo build -p dirsql --features cli
# A `.py` config no longer serves tables; the binary reports a TOML parse error.
printf 'app = 1\n' > /tmp/dirsql-a3.py
cargo run -q -p dirsql --features cli -- --config /tmp/dirsql-a3.py query "SELECT 1"
# expected: exit 1 with a "failed to load config" / TOML parse diagnostic on stderr
```

### CLI: Python `DirSQL(...)` no longer guards `(None, None)` (#260)

#### Summary

The Python `DirSQL(...)` constructor no longer raises `TypeError` when neither
`root` nor `config` is supplied — that check is delegated to the Rust core and
now surfaces from `await db.ready()` / `db.query(...)`, matching Rust and the
TypeScript SDK. Affects any Python caller relying on the constructor-time
`TypeError`. (The native-config root-defaulting and nested-`config=` rejection
that originally shipped under #260 are gone with the `interpret` removal in
#323 / #324 / #325.)

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python `DirSQL()` with neither `root` nor `config` | raises `TypeError` at construction | constructs; the "no root" error surfaces from `await db.ready()` / `query()` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Python `DirSQL(...)` defers the "no root or config" error from construction
  time to readiness time (`await db.ready()` / `query()`); the raised exception
  is the core's error, not a `TypeError`.

#### Verification

```bash
cd packages/python
uv run python -c "from dirsql import DirSQL; DirSQL(); print('constructed without TypeError')"
# expected: constructed without TypeError
```

### Rust SDK: extension-loading review followup (#225)

#### Summary

Code review of the unreleased SQLite-extension-loading feature (#225) added a
dedicated `DirSqlError::Extension` variant — load failures previously surfaced
as `DirSqlError::Core(DbError::Sqlite(_))`. Exhaustive matches on `DirSqlError`
are affected; it is a simple update.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Exhaustive `match` on `DirSqlError` | (no `Extension` arm) | add `DirSqlError::Extension { .. }` (or a `_` arm) |
| Error from a failed extension load | `DirSqlError::Core(DbError::Sqlite(_))` | `DirSqlError::Extension { path, source }` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A failed extension load now surfaces as `DirSqlError::Extension { path,
  source }` (naming the library) instead of a generic
  `DirSqlError::Core(DbError::Sqlite(_))`.

#### Verification

```bash
cargo test -p dirsql --test extensions
```

Expected: passes — extension loading works and a failed load surfaces the
dedicated `DirSqlError::Extension` variant.

### Python SDK: `DirSQL(extensions=...)` (#229)

#### Summary

The Python `DirSQL` constructor gains an additive `extensions` parameter (a list
of `{"path", "entrypoint"?}` dicts) that loads SQLite extensions onto the
connection at startup, marshaled into the shared Rust core. The constructor
parameter is backward compatible (it defaults to no extensions).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Loading a SQLite extension from the Python SDK | _not available — only `[[dirsql.extension]]` config-file entries_ | `DirSQL(root, extensions=[{"path": "...", "entrypoint": "..."}])` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._ The parameter is additive and defaults to no extensions.

#### Verification

```bash
cd packages/python
uv run python -c "from dirsql import DirSQL; DirSQL('.', extensions=[]); print('accepts extensions=')"
# expected: accepts extensions=
```

### TypeScript SDK: `new DirSQL({ extensions })` (#230)

#### Summary

The TypeScript `DirSQL` constructor gains an additive `extensions` option (an
array of `{ path, entrypoint? }` objects) that loads SQLite extensions onto the
connection at startup, marshaled through the napi binding into the shared Rust
core. The constructor option is backward compatible (it defaults to no
extensions).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Loading a SQLite extension from the TypeScript SDK | _not available — only `[[dirsql.extension]]` config-file entries_ | `new DirSQL({ root, extensions: [{ path: "...", entrypoint: "..." }] })` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._ The option is additive and defaults to no extensions.

#### Verification

```bash
cd packages/ts
pnpm build
node --input-type=module -e "import { DirSQL } from './dist/index.js'; const db = new DirSQL({ root: '.', extensions: [] }); console.log('accepts extensions'); db.ready.catch(() => {});"
# expected: accepts extensions
```

### Rust SDK: code-review followup (#218)

#### Summary

Code review of the Rust core (#218) surfaced three changes with behavior
or API impact: `DirSqlError::{Watch, Matcher, Config}` move from tuple to
struct variants so they can carry an underlying `source()`; the `_ext`
stat virtual stops lowercasing the extension; and `persist::
PARSER_VERSIONS_JSON` drops the legacy parser-versions list (per-format
parsing was removed in #169). The first is API-shape; the second and
third are runtime-behavior changes with no API change. Each is
independently opt-out via simple `LOWER()` SQL / pattern-match update.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Pattern-match on `DirSqlError::Watch` | `DirSqlError::Watch(msg)` | `DirSqlError::Watch { message, source }` (or `DirSqlError::Watch { .. }` to ignore fields) |
| Pattern-match on `DirSqlError::Matcher` | `DirSqlError::Matcher(msg)` | `DirSqlError::Matcher { message, source }` (or `{ .. }`) |
| Pattern-match on `DirSqlError::Config` | `DirSqlError::Config(msg)` | `DirSqlError::Config { message, source }` (or `{ .. }`) |
| Reading `_ext` for `Photo.JPG` | `Value::Text("jpg")` | `Value::Text("JPG")` (use `LOWER(_ext)` in SQL for the old behavior) |
| Persistent cache from a build before this change | Loads with the legacy parser-versions string | Rejected by `meta_is_compatible`; cache cleanly rebuilds on next startup |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **`_ext` now preserves the original file extension's case.** Queries that
  matched `_ext = 'jpg'` for files named `Photo.JPG` will return zero rows
  after the upgrade. Wrap the SQL in `LOWER(_ext) = 'jpg'` (or `_ext IN
  ('JPG', 'jpg', 'Jpg')`) to recover the old behavior, or rely on the
  case-sensitive contract going forward.
- **`DirSQL::query` no longer leaks `_dirsql_*` tracking columns when the
  column name appears only inside a comment or string literal.** Programs
  that intentionally exploited the substring-match leak (very unlikely)
  must instead name the column in the projection — e.g.
  `SELECT _dirsql_file_path FROM t` — to receive it.
- **Persistent on-disk caches written by older builds are rebuilt on first
  startup.** The change to `PARSER_VERSIONS_JSON` triggers
  `meta_is_compatible` to reject the cache as incompatible, which is the
  documented reconcile path. No data loss; the rebuild is automatic.

#### Verification

After upgrading, all four behaviors above can be checked in a few lines.
Spin up a tempdir with a single uppercase-extension file:

```bash
mkdir -p /tmp/dirsql-verify && cd /tmp/dirsql-verify
touch Photo.JPG
cat >.dirsql.toml <<'EOF'
[[table]]
ddl  = "CREATE TABLE pics (_ext TEXT)"
glob = "**/*"
EOF
```

Then in Rust:

```rust
let db = dirsql::DirSQL::from_config_path("/tmp/dirsql-verify/.dirsql.toml")?;
// _ext preserves case.
let rows = db.query("SELECT _ext FROM pics")?;
assert_eq!(rows[0]["_ext"], dirsql::Value::Text("JPG".into()));
// Comments don't leak _dirsql_file_path.
let rows = db.query("SELECT * FROM pics /* _dirsql_file_path */")?;
assert!(!rows[0].contains_key("_dirsql_file_path"));
```

### CLI launcher directories renamed (`_cli/` -> `cli/`, `src/bin/` -> `src/cli/`)

#### Summary

The Python `dirsql/_cli/` package and the TypeScript `packages/ts/src/bin/`
directory are both renamed to `cli/`. This is a cross-SDK consistency cleanup:
Python's leading underscore was misleading (the directory holds the public
console-script entry point, not internal-only code), and the TypeScript `bin/`
naming did not match Python's `_cli/`. The user-facing `dirsql` command on
PATH is unchanged; only the internal package layout and the published
metadata that points at it move.

Affects:

- **Python:** `[project.scripts] dirsql = "dirsql._cli.main:main"` is now
  `dirsql = "dirsql.cli.main:main"`. Anyone importing `dirsql._cli.*`
  directly (the leading underscore signalled "do not") must update their
  import path.
- **TypeScript:** the npm `bin` field on the `dirsql` package now points at
  `dist/cli/dirsql.js` instead of `dist/bin/dirsql.js`. The `dirsql`
  shim in `node_modules/.bin/` is unchanged.
- **End users running `dirsql ...`:** no change.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python module imports | `from dirsql._cli.main import main` | `from dirsql.cli.main import main` |
| Python `[project.scripts]` (wheel metadata only; not a consumer-visible API) | `dirsql = "dirsql._cli.main:main"` | `dirsql = "dirsql.cli.main:main"` |
| npm `bin` field (package metadata; not a consumer-visible API) | `"dirsql": "dist/bin/dirsql.js"` | `"dirsql": "dist/cli/dirsql.js"` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._ The `dirsql` command on PATH behaves identically before and after
the rename; only the internal module path used by the wheel's console-script
entry and the npm package's `bin` field have moved.

#### Verification

After upgrading, the `dirsql` command still resolves and runs:

```bash
dirsql --version
# expected: dirsql <version>
```

Python consumers who imported the (underscore-prefixed) launcher modules
directly should update the import path:

```bash
python -c "from dirsql.cli.main import main; print('ok')"
# expected: ok
```

### Python 3.10 support dropped

#### Summary

dirsql's `requires-python` is raised from `>=3.10` to `>=3.11`. `pip` and
`uv` will refuse to install dirsql 0.3.6+ on Python 3.10; 3.10 wheels are
no longer published. This affects only the Python SDK (`dirsql` on PyPI);
the Rust crate and npm package are unchanged.

The driver is release tooling, not a runtime API change. putitoutthere's
multi-version wheel build (#369) fans one wheel row per `requires-python`
version, and its `bundle_cli` wheel-content verify step runs `import
tomllib` — a stdlib module only on CPython >= 3.11 — so the 3.10 row
crashes the release build. Raising `requires-python` removes the 3.10
row. Support can be restored once the upstream verify step no longer
depends on `tomllib`.

#### Required changes

| Before | After |
|--------|-------|
| `pip install dirsql` on Python 3.10 | Upgrade to Python >= 3.11, then `pip install dirsql` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

Installation on Python 3.10 now fails at resolve time (`pip` reports the
package requires a different Python) instead of installing. No change for
Python 3.11+.

#### Verification

On Python 3.11 or newer: `pip install dirsql` resolves and installs as
before. On Python 3.10: `pip install dirsql` exits with
`Requires-Python >=3.11`.

### Content parsing removed; `[table.columns]` / `format` / `each` no longer recognized

#### Summary

dirsql's scope is narrowed to its actual purpose: bridging a local filesystem
to a SQL index. Content interpretation — frontmatter, JSON dot-paths, CSV
parsing, the whole `Format` zoo — is no longer dirsql's job. The `parser.rs`
module and every related symbol are deleted; the `[table.columns]`, `format`,
and `each` keys are no longer part of the `.dirsql.toml` grammar. Affects
every consumer that used `from_config` / `config=` / `new DirSQL(configPath)`
to point at JSON, JSONL, CSV, TSV, TOML, YAML, or markdown-with-frontmatter
files. Closes [#169](https://github.com/thekevinscott/dirsql/issues/169).

Programmatic `Table::new(...)` consumers are unaffected at the call-site
level: their extract callbacks already do their own parsing. They do however
gain auto-injection of glob captures and stat virtuals into each row (see
"Behavior changes without code changes" below).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` `[[table]]` for parsed content | `ddl = "CREATE TABLE items (name TEXT, price REAL)"` + `glob = "items/*.json"` (relied on JSON parsing) | Move parsing into a programmatic `Table` whose `extract` parses the bytes in your host language. The `.dirsql.toml` entry for filesystem-fact-only tables stays as `ddl = "CREATE TABLE items (_path TEXT, _basename TEXT, ...)"` + `glob`. |
| `.dirsql.toml` `format = "..."` | `format = "json"` (hard requirement when extension didn't match) | Key is no longer recognized. Drop it. To opt into content parsing, write a programmatic `Table` instead. |
| `.dirsql.toml` `each = "..."` | `each = "data.items"` (dot-path navigation into JSON/YAML/TOML) | Key is no longer recognized. Drop it. Use a programmatic `Table` whose extract walks the structure (e.g. `json.loads(content)["data"]["items"]`). |
| `.dirsql.toml` `[table.columns]` | `[table.columns]\ndisplay_name = "metadata.author.name"` | Block is no longer recognized. Drop it. To project nested values into columns, do it in a programmatic `Table` extract. |
| Glob captures in `[[table]]` | Only worked when `[table.columns]` referenced them or relied on implicit dispatch | Captures are auto-injected as columns by name (`thread_id` from `posts/{thread_id}/*.md`) when the DDL declares them. No `[table.columns]` mapping required. |
| `DirSqlError::NoFormat`, `ConfigError::UnknownFormat` (Rust) | Public error variants | Removed. Catch the parent `DirSqlError` / `ConfigError` instead. |

Worked example. Before:

```toml
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT)"
glob = "comments/{thread_id}/index.jsonl"
```

After (Python — content parsing moves into the user's code):

```python
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "/path/to/root",
    tables=[
        Table(
            ddl="CREATE TABLE comments (thread_id TEXT, body TEXT)",
            glob="comments/{thread_id}/index.jsonl",
            extract=lambda path: [
                {"body": json.loads(line)["body"]}
                for line in open(path, encoding="utf-8").read().splitlines()
                if line
            ],
        )
    ],
)
```

`thread_id` is auto-injected from the glob capture; the user's extract only
returns `{"body": ...}`.

#### Deprecations removed

_None._ The removed keys (`format`, `each`, `[table.columns]`) and error
variants (`NoFormat`, `UnknownFormat`) were never deprecated; they are
removed in a single release as part of the scope change.

#### Behavior changes without code changes

- **Filesystem-fact auto-injection** is now applied uniformly to every row,
  whether produced by a programmatic or config-defined `Table`. For each
  row the core merges in:
  - glob path captures by capture name (e.g. `thread_id`),
  - stat virtuals under reserved `_`-prefixed names (`_path`, `_basename`,
    `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`).
  Auto-injected keys are filtered to the columns declared in the table's
  DDL, so a strict-mode table with a minimal DDL is not broken by virtuals
  it didn't ask for. User-extract values win over auto-injected values when
  keys collide.

  *Impact on existing programmatic consumers:* if your DDL happens to
  declare a column whose name matches a glob capture or one of the stat
  virtuals (e.g. you had `CREATE TABLE foo (_path TEXT, ...)` in the DDL
  and your extract did **not** populate `_path`), the column is now
  populated automatically. If your extract does populate it, your value
  wins — no change in observable behavior.

- **`.dirsql.toml` files that still contain `format = "..."`, `each =
  "..."`, or `[table.columns]` blocks** parse without error (TOML's default
  permissive deserialization ignores unknown keys). The keys are silently
  dropped. Tables produce filesystem-fact rows regardless. If you relied on
  parsed content, you will see all-NULL or all-default values until you
  migrate to a programmatic `Table` (see "Required changes" above).

#### Verification

```bash
# 1. Confirm the parser module no longer exists in the dependency.
cargo tree -p dirsql --target-dir /tmp/dirsql-verify | grep -E '\bcsv\b|\bserde_yaml\b' \
  && echo 'FAIL: csv or serde_yaml still in tree' || echo 'OK: parser deps removed'

# 2. Confirm `format`/`each`/`[table.columns]` are silently ignored.
cat > /tmp/legacy.toml <<'TOML'
[[table]]
ddl  = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
format = "json"
each   = "items"
[table.columns]
old = "metadata.name"
TOML
# Parses without error; the table produces filesystem-fact rows.

# 3. Confirm filesystem-fact auto-injection on a config-defined table.
mkdir -p /tmp/dirsql-fs/posts/abc
echo '{}' > /tmp/dirsql-fs/posts/abc/hello.md
cat > /tmp/dirsql-fs/.dirsql.toml <<'TOML'
[[table]]
ddl  = "CREATE TABLE posts (thread_id TEXT, _basename TEXT, _size INTEGER)"
glob = "posts/{thread_id}/*.md"
TOML
# A query of `SELECT thread_id, _basename, _size FROM posts` returns one
# row: ("abc", "hello.md", 3).
```

---

### `extract` callbacks no longer receive file content

#### Summary

The `extract` callback on a programmatic `Table` (Rust), `Table` (Python),
or `TableDef` (TypeScript) changed from a two-argument callback
`(path, content)` to a one-argument callback `(path)`. The single argument
is the **absolute filesystem path** of the matched file (previously the
first argument was the root-relative path). `dirsql` no longer reads file
bodies during the initial scan or the watch loop, so a callback that needs
file content must read it itself. Affects every consumer that registers a
programmatic table with an `extract` callback in any of the three SDKs.
Consumers who only use `.dirsql.toml` config files are unaffected — config
tables never had a user-authored `extract`. The change removes a vestigial
eager UTF-8 read left over from the content-parsing feature deleted in
[#169](https://github.com/thekevinscott/dirsql/issues/169); a side effect
is that a table glob may now match binary (non-UTF-8) files without
aborting the build. Closes part of
[#184](https://github.com/thekevinscott/dirsql/issues/184).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python `extract` (uses content) | `extract=lambda path, content: [json.loads(content)]` | `extract=lambda path: [json.loads(open(path, encoding="utf-8").read())]` |
| Python `extract` (ignores content) | `extract=lambda path, content: [...]` | `extract=lambda path: [...]` |
| Rust `extract` (uses content) | `Table::new(ddl, glob, \|_path, content\| parse(content))` | `Table::new(ddl, glob, \|path\| parse(&std::fs::read_to_string(path).unwrap()))` |
| Rust `extract` (ignores content) | `Table::new(ddl, glob, \|_path, _content\| ...)` | `Table::new(ddl, glob, \|_path\| ...)` |
| TypeScript `extract` (uses content) | `extract: (path, content) => [JSON.parse(content)]` | `extract: (path) => [JSON.parse(readFileSync(path, "utf8"))]` |
| TypeScript `extract` (ignores content) | `extract: (path, content) => [...]` | `extract: (path) => [...]` |
| Path argument semantics | first argument was the root-relative path | first (only) argument is the absolute filesystem path |

#### Deprecations removed

_None._ The two-argument signature was never deprecated; it is replaced in a
single release alongside the related zero-config work in #184.

#### Behavior changes without code changes

- A table glob that matches a binary / non-UTF-8 file no longer aborts
  construction. Previously `dirsql` eagerly read every matched file as UTF-8
  text and surfaced an `InvalidData` error; it now never reads file bodies
  itself, so binary files are indexed for their filesystem facts without
  error.
- The path handed to `extract` is now absolute rather than root-relative.
  Callbacks that derived columns from the path via `Path`/`os.path`
  component accessors (`parent`, `file_name`/`basename`) are unaffected;
  callbacks that compared the path against a hard-coded relative string must
  be updated.

#### Verification

```bash
# A programmatic table whose glob matches a binary file builds cleanly and
# the callback receives an absolute path it can open itself.
python - <<'PY'
import tempfile, os
from dirsql import DirSQL, Table

root = tempfile.mkdtemp()
open(os.path.join(root, "logo.png"), "wb").write(b"\xff\xd8\xff\x00")

db = DirSQL(root, tables=[Table(
    ddl="CREATE TABLE assets (_basename TEXT)",
    glob="*.png",
    extract=lambda path: (os.path.isabs(path) or 1/0) and [{}],
)])
import asyncio; asyncio.run(db.ready())
print(asyncio.run(db.query("SELECT _basename FROM assets")))
# expected: [{'_basename': 'logo.png'}]
PY
```

---

### Zero-config run serves a default `files` table

#### Summary

Running the `dirsql` server (no subcommand) in a directory without a
`.dirsql.toml` used to leave the server degraded: it bound the port but
every `POST /query` returned HTTP 503 with `config not found`. It now
indexes the directory with a built-in `files` table -- one row per file,
columns drawn entirely from filesystem facts -- and serves queries
normally. Affects only the CLI server's no-config path; consumers who
always run with a `.dirsql.toml`, and all programmatic SDK consumers, are
unaffected. Part of
[#184](https://github.com/thekevinscott/dirsql/issues/184).

#### Required changes

_None._ The change is additive for anyone who already ships a `.dirsql.toml`
-- a present config fully overrules the default.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `dirsql` started in a directory without a `.dirsql.toml`: previously every
  `POST /query` returned `503 Service Unavailable` with
  `{"error":"config not found at ./.dirsql.toml"}`; now the server is
  `Ready` and `POST /query` runs against a default `files` table (one row
  per file, columns `_path`, `_basename`, `_dir`, `_ext`, `_size`,
  `_mtime`, `_ctime`). Tooling that probed for the 503 to detect "no
  config" must instead check for the `files` table or for the presence of a
  `.dirsql.toml`. The 503 path still applies when a config file exists but
  fails to load.

#### Verification

```bash
cd "$(mktemp -d)"
echo hi > note.txt
dirsql query "SELECT _basename FROM files"
# expected: [{"_basename":"note.txt"}]
```

---

### Release pipeline migrated to `putitoutthere`

#### Summary

The release process is now driven by [putitoutthere](https://github.com/thekevinscott/putitoutthere). No SDK call sites change; the migration is observable in tag layout, npm package layout, and CI configuration. Consumers installing via `pip install dirsql` / `cargo add dirsql` / `npm install dirsql` see no behavioral difference at install time. Operators reading release tags or pinning npm sub-packages by name need to update their references.

#### Required changes

| Surface | Before | After |
|---|---|---|
| Git tag for a release | one shared tag `v{version}` | three per-package tags `dirsql-rust-v{version}`, `dirsql-py-v{version}`, `dirsql-npm-v{version}` |
| npm CLI sub-packages | `@dirsql/cli-<short-slug>` (e.g. `@dirsql/cli-linux-x64-gnu`) | `@dirsql/cli-{triple}` (e.g. `@dirsql/cli-linux-x64-gnu`) — same scheme, retained via `name` template |
| npm napi sub-packages | `@dirsql/lib-<short-slug>` | `@dirsql/lib-{triple}` — same scheme, retained via `name` template |
| Release trigger | scheduled cron + immediate-on-push (toggle via `RELEASE_STRATEGY` repo var) | every push to `main` whose changes match a package's `globs` |
| Skip a release | `[no-release]` in commit message | `release: skip` trailer in commit body |
| Bump type | `workflow_dispatch` input (`patch` / `minor`) | `release: <bump>` trailer in commit body (default `patch`) |
| Publish auth | bootstrap `NPM_TOKEN` + `crates-io-auth-action` + PyPI TP | OIDC trusted publishers on all three registries; no long-lived tokens reachable from the workflow |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **PyPI wheels temporarily ship without the `dirsql` CLI binary.** The previous pipeline cross-compiled the Rust binary per target and bundled it into each wheel, so `pip install dirsql` shipped a working `dirsql` command. Putitoutthere v0.2.3 has the `[package.bundle_cli]` recipe in its schema but no workflow step that builds + stages the binary, so the block is dropped from `putitoutthere.toml` for now. `dirsql._cli.main` still installs as a Python module but raises `FileNotFoundError` if invoked, pointing at `cargo install dirsql --features cli` or `npx dirsql` as alternate install paths. To restore: re-add `[package.bundle_cli]` once the upstream gap is closed and `[project.scripts] dirsql = "dirsql._cli.main:main"` to `packages/python/pyproject.toml`.
- **Per-SDK selective publishing.** The `workflow_dispatch` `publish_python` / `publish_rust` / `publish_js` toggles are gone; package selection now flows through `release: <bump> [<pkg-name>, ...]` trailers (per-package names: `dirsql-rust`, `dirsql-py`, `dirsql-npm`).
- **Auto-rollback on partial publish failure** is no longer performed. The previous pipeline deleted the tag if both PyPI and crates.io publishes failed; under putitoutthere, a partial failure leaves the published artifacts in place and re-runs are idempotent (each handler's first move is `isPublished`, which short-circuits cleanly on already-published versions).
- **GitHub Release notes** are still auto-generated (`gh release create --generate-notes`) but the Release is now created by the reusable workflow, not the consumer's `publish.yml`.
- **Dry-run mode** is removed. The plan job is side-effect-free; inspect the matrix output on a feature branch to preview a release.

#### Verification

```
# 1. The new caller workflow lints clean.
yamllint .github/workflows/release.yml

# 2. The toml parses and the plan resolves.
#    (Locally — putitoutthere's `plan` is pure over (config + git state).)
npx -y putitoutthere@0.2 plan

# 3. Trusted publishers on all three registries point at this filename.
#    Expected entry on each:
#      Repository: thekevinscott/dirsql
#      Workflow:   release.yml
#      Environment: release
#    PyPI:    https://pypi.org/manage/project/dirsql/settings/publishing/
#    crates:  https://crates.io/crates/dirsql/settings
#    npm:     https://www.npmjs.com/package/dirsql/access
#             — plus one per per-platform package (see PR body).
```

<!--
When a PR introduces a breaking change, a deprecation removal, or a
behavior-only change, copy the template block below into the `## [Unreleased]`
section and fill it in. When a release is cut, rename `## [Unreleased]` to
`## [vX.Y.Z] - YYYY-MM-DD` and start a fresh Unreleased section above it.

Migration entries are required for:
  - Breaking API changes (signatures, names, return types, config keys)
  - Removal of a previously deprecated symbol
  - Behavior changes that keep the same API (exit codes, event payloads,
    on-disk layouts, default values, tag formats)

Migration entries are NOT required for purely additive changes, bug fixes that
restore documented behavior, or changes that are internal-only.
-->

---

## Migration entry template

Copy this block in full. Every subsection is required; if a subsection does
not apply, keep the heading and write `_None._`.

### `<Short title of the change>`

#### Summary

One paragraph. State what broke, which SDKs and call sites are affected, and
why the change was made (bug, parity, redesign, dependency upgrade). A reader
who lands here from a failing build should be able to decide in 30 seconds
whether this migration is the cause.

#### Required changes

A table of before/after snippets covering every affected surface: config
files, CLI flags, action inputs, function signatures, return types. One row
per distinct surface. Include per-SDK snippets where they differ.

| Surface | Before | After |
| ------- | ------ | ----- |
| `<e.g. Python DirSQL.open>` | `<prior call site>` | `<new call site>` |
| `<e.g. CLI flag>` | `<old flag>` | `<new flag>` |

#### Deprecations removed

Anything previously marked deprecated that is now gone. Consumers on the
prior version should have seen warnings; this section tells them which of
those warnings have become hard errors.

- `<deprecated symbol>` (deprecated in `<version>`) — removed; use `<replacement>`.

#### Behavior changes without code changes

Same API, different runtime behavior. Cover exit codes, tag/ID formats,
on-disk layouts, event payloads, retry behavior, default values. Each bullet
names the surface and describes the old vs. new behavior concretely.

- `<surface>`: previously `<old behavior>`; now `<new behavior>`. `<impact on
  consumer code, if any>`.

#### Verification

A concrete recipe a consumer can run to confirm the upgrade worked. Prefer a
dry-run or read-only command plus expected output; do not require them to
mutate real data.

```bash
<command>
# expected: <output>
```
