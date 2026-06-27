---
canonical: https://thekevinscott.github.io/dirsql/cli/
---

# Using `dirsql` from the CLI

> Online: <https://thekevinscott.github.io/dirsql/cli/>

`dirsql` ships a command-line interface that starts an HTTP server exposing
the same indexing, querying, and watching functionality as the SDK — no host
language required. Run it in any directory to query your files over HTTP:
with no config it serves a default `files` table, and a
[config file](./config.md) defines custom tables.

Everything you need to run `dirsql` as a CLI lives in this section:

- **[Installation](#installation)** — get the `dirsql` binary.
- **[Running the Server](./server.md)** — subcommands and flags.
- **[Generating a Config (`init`)](./init.md)** — scaffold a `.dirsql.toml`.
- **[Configuration File](./config.md)** — the `.dirsql.toml` format and the
  `.py` / `.js` native-language alternative. Custom tables
  are defined through a config file; without one, the server runs in
  [zero-config mode](./server.md#zero-config-mode).
- **[HTTP API](./http-api.md)** — the `POST /query` and `GET /events`
  endpoints, status codes, and event streaming.

## Installation

::: code-group

```bash [npm]
npx dirsql
```

```bash [PyPI]
uvx dirsql
```

```bash [Cargo]
# Installs the binary only (the `cli` feature is non-default)
cargo install dirsql --features cli
dirsql
```

:::

::: tip For Rust library consumers
The `cli` feature is **opt-in**. Adding `dirsql` as a library dependency
(`cargo add dirsql`) pulls no CLI dependencies — only the core library. See the
[Rust library README](https://github.com/thekevinscott/dirsql/tree/main/packages/rust)
for the library-vs-CLI feature split.
:::

## Quick start

From any directory containing your files (no config needed):

```bash
dirsql

$ Running at localhost:7117
```

Then query it over HTTP:

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT COUNT(*) AS n FROM files"}' \
  | jq
```

See [Running the Server](./server.md) for flags and [HTTP API](./http-api.md)
for the full endpoint reference.
