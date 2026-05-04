---
canonical: https://thekevinscott.github.io/dirsql/guide/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/guide/init>

`dirsql init` writes a `.dirsql.toml` for you instead of asking you to
author one by hand. Under the hood it spawns a short-lived sub-agent
via the [Anthropic Agent SDK](https://docs.claude.com/en/api/agent-sdk),
gives it filesystem-read tools, and asks it to inspect your files and
produce a config. The agent sees the actual content of your data files
-- not just the extensions -- so the resulting DDL has real column
names and types, ready to query.

The LLM call happens **once, at init time**. The generated `.dirsql.toml`
is a static file: `dirsql` itself never calls a model when you run
`query()` or `watch()`.

## Quick start

```bash
cd my-project
dirsql init
```

That's it. The agent inspects the directory, samples representative
files, picks table names + globs, infers DDL from observed JSON keys
(or CSV headers, or YAML keys, or frontmatter fields), and writes
`.dirsql.toml` at the project root.

```text
$ dirsql init
dirsql init: launching schema-inference agent (model: claude-sonnet-4-6)…
dirsql init: agent inspected 14 files across 3 patterns
dirsql init: wrote ./.dirsql.toml (3 table(s))
```

The output is a working config you can run `dirsql` against immediately;
no manual fix-up required. Refine table names, drop columns, or add
`each` / `columns` mappings as you see fit.

## What the agent actually does

The sub-agent is launched with three tools (read-only, scoped to the
target root):

- `list_files(glob)` -- enumerate files matching a glob
- `read_file(path, max_bytes)` -- read up to N bytes of a file's content
- `propose_config(toml)` -- emit the final `.dirsql.toml` and exit

Its system prompt instructs it to:

1. List the directory tree, ignoring common noise (`node_modules/`,
   `.git/`, `target/`, `__pycache__/`, ...).
2. Pick one or more **glob patterns** that group related files (e.g.
   `posts/*.json`, `_comments/{thread_id}/index.jsonl`).
3. Sample 1-3 files per pattern, look at their structure, and propose
   a `CREATE TABLE` DDL with column names and types drawn from the
   observed keys.
4. Decide whether `format` / `each` / `columns` overrides are needed
   (most cases need none -- format is inferred from the extension).
5. Call `propose_config(...)` exactly once with the rendered TOML.

The agent runs to completion in a single SDK session. No tool calls
happen at query time.

## Authentication

`dirsql init` reuses whatever credentials your local Claude Code is
already using:

- If `claude` is signed in (`claude /login`), `dirsql init` picks up
  the same OAuth token automatically.
- Otherwise it falls back to `ANTHROPIC_API_KEY` from the environment.
- If neither is available, `dirsql init` exits with a clear error
  pointing at <https://docs.claude.com/en/docs/claude-code/quickstart>.

There's no separate API key or config flag for the common case --
running `dirsql init` from a workstation where you already use Claude
Code "just works."

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | cwd | Directory to scan |
| `--output <path>` | `<root>/.dirsql.toml` | Where to write the generated config |
| `--force` | off | Overwrite the output file if it already exists |
| `--model <id>` | `claude-sonnet-4-6` | Override the model the sub-agent runs on |
| `--max-files <n>` | `200` | Hard cap on files the agent is allowed to enumerate |
| `--max-bytes <n>` | `4096` | Per-file content sample cap fed to the agent |
| `--dry-run` | off | Print the proposed config to stdout instead of writing it |
| `--system-prompt <path>` | (built-in) | Override the agent's system prompt (power users) |

`init` refuses to clobber an existing file unless `--force` is passed:

```text
$ dirsql init
dirsql init: ./.dirsql.toml already exists; pass --force to overwrite
```

## Reviewing the output before committing

Use `--dry-run` if you want to inspect the proposed config before it
hits disk:

```bash
dirsql init --dry-run | less
```

The proposal is deterministic for a given directory + model + system
prompt within a session, but Claude is non-deterministic across
sessions; treat the output as a starting point, not a contract. The
file is meant to be checked into your repo and edited by humans like
any other config.

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

## Why an agent and not just one prompt?

A single-shot prompt would have to either receive the entire directory
contents (blowing up the context window for non-trivial repos) or
receive a heuristic summary (which loses the signal that makes
inference good). Giving the agent its own `read_file` tool lets it
*decide* which files are worth opening based on what it has already
seen -- the same way a human would.

## Why piggyback on local Claude Code?

- **Auth comes for free.** No new credentials, no `dirsql`-specific
  API key plumbing.
- **Bring your own context limits / billing.** The init call counts
  against your existing Claude Code usage, not a separate `dirsql`
  bucket.
- **One model upgrade path.** When you bump your local Claude Code
  model, `dirsql init` follows automatically.
- **Auditable.** The agent's tool calls are visible in the Claude
  Code session log, so you can see exactly which files it inspected
  and why it picked the schema it did.
