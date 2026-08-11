# Load a SQLite extension

SQLite extensions add functions to your query surface — vector distances,
crypto digests, statistics. `dirsql` loads them at startup from a
[`[[dirsql.extension]]`](../reference/config.md#dirsql-extension) entry,
named either by file path or by installed package.

## Option A: a literal file path

Works everywhere — the `uvx`/`npx` launchers, the standalone
`cargo install` binary, and every SDK. Point `path` at the extension's
shared library (here [`sqlite-vec`](https://github.com/asg017/sqlite-vec)'s
`vec0.so`, copied into the project):

```toml
[[dirsql.extension]]
path       = "./ext/vec0.so"
entrypoint = "sqlite3_vec_init"
```

Relative paths resolve against the config file's directory. `entrypoint`
overrides the init symbol when it doesn't match the filename-derived
default — `sqlite-vec` is exactly such a case
([reference](../reference/config.md#dirsql-extension)).

The extension's functions are callable (pass the config with
[`-c`](../reference/cli.md#flags) so its `[[dirsql.extension]]` entry loads):

```bash
dirsql query "SELECT vec_version() AS vec_version" -c ./.dirsql.toml
```

```json
[{"vec_version":"v0.1.9"}]
```

## Option B: a package name

When the extension ships as a pip or npm package, name the package and let
the launcher find the loadable inside it — no path to copy or keep in sync.
How a bare name resolves (file-first probing, one-loadable rule) is defined
under [`[[dirsql.extension]]`](../reference/config.md#dirsql-extension);
what to actually write differs per runtime:

**Python (`uvx dirsql`, Python SDK).** Use the *importable module name* —
for the `sqlite-vec` pip package that is `sqlite_vec` (underscore), which
is looked up via `importlib`:

```toml
[[dirsql.extension]]
path       = "sqlite_vec"
entrypoint = "sqlite3_vec_init"
```

```bash
uvx --with sqlite-vec dirsql server -c ./.dirsql.toml
```

**Node (`npx dirsql`, TypeScript SDK).** Use the *npm package name* — but
note the named package must itself contain the loadable file. `sqlite-vec`
on npm is a meta-package whose loadable lives in a platform-specific
sub-package, so name that one (and install it) instead:

```toml
[[dirsql.extension]]
path       = "sqlite-vec-linux-x64"   # or -darwin-arm64, etc.
entrypoint = "sqlite3_vec_init"
```

If a platform-specific config is awkward, fall back to a literal path
(Option A).

**Rust (standalone binary, Rust SDK).** File paths only — there is no
interpreter to resolve package names with
([reference](../reference/config.md#dirsql-extension)).

## Notes

- Extensions add **functions**. An extension-backed virtual table cannot be
  declared as a `[[table]]` — `dirsql` tables are per-file row tables
  ([reference](../reference/config.md#dirsql-extension)).
- Loading happens before any table DDL runs, and the SQL
  `load_extension()` function is never exposed to queries
  ([reference](../reference/config.md#dirsql-extension)).
- Embedding `dirsql` in a program? The SDK constructor takes the same
  specs via its `extensions` parameter
  ([SDK reference](../reference/sdk.md#constructor)).
