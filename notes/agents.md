# Agent Notes

These notes augment the workflow rules in the top-level `AGENTS.md`. They
capture environment-specific guidance that depends on *where* the Claude Code
session is running.

## Detecting the execution environment

Claude Code distinguishes between **local** sessions (running on a developer's
own machine / terminal) and **remote** sessions (running in an Anthropic-hosted
sandbox -- "Claude Code on the web", "managed agents", or "cloud" execution).

The canonical signal is the `CLAUDE_CODE_REMOTE` environment variable:

- `CLAUDE_CODE_REMOTE=true` -- the session is running in a hosted / managed /
  cloud sandbox. Read `notes/environments/remote.md` **before doing any work**.
- Unset or any other value -- the session is local. The standard rules in
  `AGENTS.md` apply without modification.

Reference: <https://code.claude.com/docs/en/how-claude-code-works.md#execution-environments>

## Required reading order

1. Always read `AGENTS.md` (universal rules: architecture, scratch files, shell
   commands, testing philosophy).
2. **If `CLAUDE_CODE_REMOTE=true`, read `notes/environments/remote.md` next**,
   before issuing any tool calls that touch the working tree, git, or GitHub.
   Remote sessions have different identity, branching, tooling, and
   verification rules; skipping this file will cause the agent to violate
   sandbox conventions.
3. Otherwise, follow the local environment guidance.

The harness can be queried directly to confirm the environment:

```bash
echo "${CLAUDE_CODE_REMOTE:-local}"
```

If the value is `true`, treat the session as remote.
