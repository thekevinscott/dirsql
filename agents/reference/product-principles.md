# Product Principles

Maintainer decisions that bound any design. Read this before proposing a feature, a flag, or a behavior change.

## Read-only

dirsql never writes to the files it queries: no write-back, and no `UPDATE` that edits a source file. File mutations stay in the user's own scripts, and dirsql only queries the result. That guarantee is a selling point: pointing dirsql at anything is risk-free.

## The CLI is primary

The main use case is a transient one-liner, `uvx dirsql "SELECT ..."`, with nothing colocated. Judge every design by that one-liner first. SDK ergonomics come second, as parity work (`PARITY.md`).

## Globs follow bash

Path globbing matches `bash -O globstar` with `dotglob` off, exactly. That is the consensus of bash, zsh, node-glob/minimatch, and Python `glob`, with bash breaking ties. The approved divergences, each documented with its reason in the glob docs (epic #1255):

- `.gitignore` is respected by default.
- `'./'` and a named directory list one level.
- `node_modules` is skipped unless the path names it.
- Files only; no directory rows.

To check a glob change, compare its output with `bash -O globstar -O nullglob` on the same tree. A divergence with no written reason is a bug.

## One strict contract

When an interface needs a rule for authors, propose the smallest strict contract and reject everything else. No lenient parsing of several shapes, and no protocol markers that make the author serve the implementation. Drop a requirement before adding syntax to satisfy it.

## No baked-in timeouts

Add a timeout only where `timeout(1)` cannot express the bound, that is, where the unit of work is not the process lifetime. Kept: the server's per-request 408 and the per-round-trip timeout on persistent function workers.

## Never `--persist`

Never run dirsql with `--persist`, and never recommend it as the fix for a slow scan. Narrow the glob instead.

## The agent skill is a stub

`SKILL.md` holds a static name and description, and its body calls `uvx dirsql context`. Usage docs are compiled into the CLI, so they match the version that runs the query. Put version-sensitive guidance in the `dirsql context` docs, never in `SKILL.md`.

## Speed-of-light tests

Every implementation path gets a speed-of-light e2e test against the fastest fair native tool (`packages/python/tests/e2e/speed_of_light.py`; the bar is 1.1x native). When a new code path lands, propose one without being asked.

## YAGNI

Build only what was asked for. Don't design or surface features nobody requested; side effects of SQLite itself (internal FTS or vec shadow tables) are not dirsql's problem.
