# Give your coding agent the `dirsql` skill

Your coding agent answers questions about many files by reading them one at a
time. The `dirsql` skill teaches it to write one SQL query instead.

## What it does

The skill is a short `SKILL.md`. Its description tells the agent when to use
it: counting, grouping or aggregating files, "which files are ..." by size,
mtime or extension, pulling one field out of many JSON or frontmatter files,
or comparing two directories. It tells the agent not to use it to read one
known file, hunt for a literal string, or list a few names.

When the skill triggers, the agent runs `dirsql context`. That prints a guide
(usage, recipes, troubleshooting) compiled into the `dirsql` binary, so the
agent always reads the guide for the version it is about to run. See
[`dirsql context`](../reference/cli.md#dirsql-context).

## Install

```bash
npx skills add thekevinscott/dirsql
```

This uses [`skills`](https://www.npmjs.com/package/skills), a cross-agent
skill installer. It installs for the current project by default. Pass `-g` to
install for your user instead, `-a <agent>` to name
agents (repeatable), and `-y` to skip the prompts.

| Scope | Skill lands in | Claude Code link |
|---|---|---|
| Project (default) | `.agents/skills/dirsql` | `.claude/skills/dirsql` |
| User (`-g`) | `~/.agents/skills/dirsql` | `~/.claude/skills/dirsql` |

Codex, Cursor, Gemini CLI, Amp, Cline and other agents that read
`.agents/skills` use the shared copy directly. Agents with their own skills
directory, such as Claude Code, get a symlink to it (`--copy` copies instead).
`npx skills add --help` lists the rest.

## Runners

The skill works with whichever runner you have. Its `allowed-tools` lets the
agent run these commands without a permission prompt:

| Runner | Command the agent runs |
|---|---|
| `uv` | `uvx dirsql ...` |
| `npm` | `npx -y dirsql ...` |
| `cargo` | `dirsql ...` after `cargo install dirsql --features cli` |

The grant is `Bash(uvx dirsql:*)`, `Bash(npx -y dirsql:*)` and
`Bash(dirsql:*)`.

## Check, update, remove

```bash
npx skills list        # installed skills and their agents (-g for user scope)
npx skills update      # refresh to the latest from GitHub
npx skills remove dirsql
```

`update` and `remove` accept `-g` for user scope and `-y` to skip prompts.
The skill itself never needs updating to match the CLI, since the guide comes
from the installed `dirsql`.
