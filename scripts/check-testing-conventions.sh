#!/usr/bin/env bash
#
# Enforce the "every source file has a colocated unit test" convention with
# the `testing-conventions` CLI (https://github.com/thekevinscott/testing-conventions).
#
# dirsql already *describes* this rule in AGENTS.md ("Test Locations": Python
# `foo.py` -> `foo_test.py`; TypeScript `foo.ts` -> `foo.test.ts`). This script
# turns the prose into a deterministic, blocking CI gate.
#
# Exemptions for genuine entry shims (package barrels, the npm launcher) live
# in testing-conventions.toml at the repo root -- the CLI reads it by default,
# requires a `reason` on every entry, rejects stale entries, and fails on any
# *non-exempt* source file lacking a colocated test. So the gate is blocking
# and the allow-list stays honest, with no bespoke filtering here.
#
# Install the pinned CLI first (see .github/workflows/testing-conventions.yml):
#   pip install "testing-conventions==<version>"
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v testing-conventions >/dev/null 2>&1; then
  echo "error: 'testing-conventions' CLI not found on PATH." >&2
  echo "       install it with: pip install \"testing-conventions==<version>\"" >&2
  echo "       (see .github/workflows/testing-conventions.yml for the pinned version)" >&2
  exit 127
fi

# Scan each SDK's source tree -- source dirs only, never the test trees (the
# CLI would flag the test files themselves as untested). Run both languages
# even if the first fails so a single invocation reports every violation.
# `--config` defaults to ./testing-conventions.toml; passed explicitly here so
# the source of the exempt list is obvious at the call site.
rc=0
testing-conventions unit location --language python \
  packages/python/dirsql --config testing-conventions.toml || rc=1
testing-conventions unit location --language typescript \
  packages/ts/src --config testing-conventions.toml || rc=1
exit "$rc"
