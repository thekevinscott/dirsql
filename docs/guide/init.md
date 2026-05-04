---
canonical: https://thekevinscott.github.io/dirsql/guide/init
---

# Generating a config with `dirsql init`

> Online: <https://thekevinscott.github.io/dirsql/guide/init>

`dirsql init` writes a starter `.dirsql.toml` so you don't have to author one
by hand. There are two modes: a **heuristic template** (the default) and an
**LLM-assisted** mode (`--infer`).

## Template mode

```bash
dirsql init
```

Walks the current directory, groups files by parent directory + extension, and
writes one `[[table]]` entry per recognised file extension (`json`, `jsonl`,
`csv`, `tsv`, `toml`, `yaml`, `md`). Each table has a placeholder DDL of
`CREATE TABLE <name> (payload TEXT)`; you are expected to refine the column
list by hand.

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | cwd | Directory to scan |
| `--output <path>` | `<root>/.dirsql.toml` | Where to write the generated config |
| `--force` | off | Overwrite the output file if it already exists |

`init` refuses to clobber an existing file unless `--force` is passed:

```text
$ dirsql init
dirsql init: /path/.dirsql.toml already exists; pass --force to overwrite
```

## LLM-assisted mode

```bash
dirsql init --infer --print-prompt > prompt.txt
# feed prompt.txt to your LLM of choice (Claude, GPT, local model, ...)
# the LLM returns a JSON object describing the schema
dirsql init --infer --apply response.json
```

The pipeline is intentionally split in two so the LLM call lives outside
`dirsql`. You bring your own model and credentials; `dirsql` only formats the
prompt and validates the response.

### `--print-prompt`

Builds a prompt that contains the schema contract (a JSON object the LLM
should produce) plus a directory summary -- one block per inferred glob, with
file counts and content previews. The LLM is instructed to return JSON only:

```json
{
  "ignore": ["node_modules/**"],
  "tables": [
    {
      "ddl": "CREATE TABLE posts (title TEXT, author TEXT)",
      "glob": "posts/*.json",
      "format": "json",
      "each": null,
      "columns": null
    }
  ]
}
```

### `--apply <file>`

Reads a JSON response from `<file>` (or `-` for stdin), validates that
`tables` is non-empty and every entry has `ddl` + `glob`, then renders
`.dirsql.toml`. A leading ```` ```json ```` fence is tolerated in case the
LLM ignores the "JSON only" instruction.

```bash
# Pipe through Claude's CLI in one shot
dirsql init --infer --print-prompt | claude -p > response.json
dirsql init --infer --apply response.json

# Or via stdin
dirsql init --infer --apply -
```

If the response is missing `tables`, has `tables: []`, or any table is
missing `ddl`/`glob`, `dirsql init` exits non-zero with a clear error and
does **not** write a partial config.

### Built-in HTTP client (planned)

A future revision will let `dirsql init --infer` (with no sub-flag) make the
LLM HTTP call itself. Tracked separately from issue #96. Until then, prefer
the two-step `--print-prompt` / `--apply` pipeline above.

## Why split the pipeline?

- **No LLM in the hot path.** The generated `.dirsql.toml` is a static file;
  `dirsql` itself never calls an LLM at query / watch time.
- **Bring your own model.** The framework is provider-agnostic. Any LLM that
  can return JSON works.
- **Testable.** The CLI's own tests exercise the full path with offline JSON
  fixtures -- no API keys, no network.
- **Auditable.** You see exactly what the LLM proposed before it lands on
  disk, and the JSON response file can be checked into a PR.
