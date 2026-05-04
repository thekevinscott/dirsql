---
canonical: https://thekevinscott.github.io/dirsql/guide/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/guide/init>

`dirsql init` writes a `.dirsql.toml` for you instead of asking you to
author one by hand. Under the hood it shells out to `claude` and asks
it to inspect your files and produce a config. The agent sees the
actual content of your data files -- not just the extensions -- so the
resulting DDL has real column names and types, ready to query.

The LLM call happens **once, at init time**. The generated `.dirsql.toml`
is a static file: `dirsql` itself never calls a model when you run
`query()` or `watch()`.

## Quick start

```bash
cd my-project
dirsql init
```

That's it. `claude` inspects the directory, picks table names + globs,
infers DDL from observed JSON keys (or CSV headers, or YAML keys, or
frontmatter fields), and writes `.dirsql.toml` at the project root.

```text
$ dirsql init
dirsql init: invoking claude…
dirsql init: wrote ./.dirsql.toml
```

The output is a working config you can run `dirsql` against immediately;
no manual fix-up required. Refine table names, drop columns, or add
`each` / `columns` mappings as you see fit.

## Authentication

`dirsql init` requires the `claude` CLI to be installed and signed in.
It uses whatever credentials `claude` itself uses; there's no separate
API key or config flag.

If `claude` is not on `PATH`, `dirsql init` exits with a clear error
pointing at <https://docs.claude.com/en/docs/claude-code/quickstart>.

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | cwd | Directory to scan |
| `--output <path>` | `<root>/.dirsql.toml` | Where to write the generated config |
| `--force` | off | Overwrite the output file if it already exists |

`init` refuses to clobber an existing file unless `--force` is passed:

```text
$ dirsql init
dirsql init: ./.dirsql.toml already exists; pass --force to overwrite
```

## Reviewing the output before committing

Claude is non-deterministic across sessions; treat the generated
`.dirsql.toml` as a starting point, not a contract. The file is meant
to be checked into your repo and edited by humans like any other
config.

## When the agent gets it wrong

Two failure modes are worth calling out:

**Over-eager column inference.** If a JSON file has a deeply nested
or sparse schema, the agent may propose columns you don't actually
care about. Drop them by hand; `dirsql` ignores extra keys by default
(see [Configuration File](./config.md#strict-mode)).

**Missed `each` / `columns`.** Path captures (`{thread_id}` in a glob)
and dot-path navigation (`each = "data.items"`) require the agent to
notice patterns it might miss on a small directory. If your schema
needs them, add them by hand after init -- the docs at
[Configuration File](./config.md) have full examples.

`dirsql init` is a starter, not a replacement for understanding your
own data layout. The win is that the starter is *real working SQL*
on day one, not a `payload TEXT` placeholder.

## Why piggyback on local `claude`?

- **Auth comes for free.** No new credentials, no `dirsql`-specific
  API key plumbing.
- **Bring your own billing.** The init call counts against your
  existing Claude Code usage, not a separate `dirsql` bucket.
- **No agent loop to maintain.** `claude` already handles tool
  dispatch, retries, and model upgrades; `dirsql` just runs it.
