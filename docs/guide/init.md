---
canonical: https://thekevinscott.github.io/dirsql/guide/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/guide/init>

`dirsql init` generates a `.dirsql.toml` by running `claude` over the target directory.

## Usage

```bash
$ dirsql init
[claude streams its tool calls and reasoning here]
wrote ./.dirsql.toml
```

`init` will not overwrite an existing config without `--force`.

## Flags

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | cwd | Directory to scan |
| `--output <path>` | `<root>/.dirsql.toml` | Output path |
| `--force` | off | Overwrite if the output exists |

## Authentication

Requires `claude` on `PATH` and signed in. There is no separate API key. If `claude` is missing, `dirsql init` exits with a link to <https://docs.claude.com/en/docs/claude-code/quickstart>.
