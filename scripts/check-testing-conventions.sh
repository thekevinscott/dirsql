#!/usr/bin/env bash
#
# Enforce the "every source file has a colocated unit test" convention with
# the `testing-conventions` CLI (https://github.com/thekevinscott/testing-conventions).
#
# dirsql already *describes* this rule in AGENTS.md ("Test Locations": Python
# `foo.py` -> `foo_test.py`; TypeScript `foo.ts` -> `foo.test.ts`). This script
# turns the prose into a deterministic, blocking CI gate.
#
# The CLI has no per-file ignore flag yet, so we run it across each SDK's
# source tree and fail on every reported violation EXCEPT an explicit,
# documented allow-list (EXEMPT below). Any *new* uncovered source file
# therefore breaks the build -- the gate is blocking, not advisory -- while a
# genuine entry shim stays exempt without shipping a throwaway test for it.
#
# Keep EXEMPT minimal and in lockstep with the coverage-omit lists it mirrors
# (packages/ts/vitest.config.ts, packages/python/pyproject.toml). Remove an
# entry the moment its file gains a real colocated test.
#
# Install the pinned CLI first (see .github/workflows/testing-conventions.yml):
#   pip install "testing-conventions==<version>"
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# "<language> <source-tree>" pairs to scan. Source trees only -- never the
# test trees (the CLI would flag the test files themselves as untested).
SCANS=(
  "python packages/python/dirsql"
  "typescript packages/ts/src"
)

# Files intentionally exempt from the colocated-unit-test rule. One reason per
# entry; mirror the rationale in the coverage-omit config.
EXEMPT=(
  # npm `bin` launcher shim: a ~5-line module that imports `main` and invokes
  # it at load. Not a unit under test; the pack-install smoke test exercises
  # it end-to-end. Mirrors the coverage exclude in packages/ts/vitest.config.ts.
  "packages/ts/src/cli/dirsql.ts"

  # --- TEMPORARY: remove when TESTING_CONVENTIONS_VERSION is bumped ----------
  # The pinned CLI (0.0.6) has no `index.ts` ignore yet, so barrel re-export
  # files (pure `export ... from "./x.js"`, nothing to unit-test) are still
  # flagged. Upstream testing-conventions ignores `index.ts` natively; once the
  # workflow pins a version that includes that fix, DELETE the two entries below
  # (and this block) -- they become redundant no-ops on the newer CLI.
  "packages/ts/src/index.ts"
  "packages/ts/src/cli/interpret/index.ts"
  # --------------------------------------------------------------------------
)

is_exempt() {
  local needle="$1" entry
  for entry in "${EXEMPT[@]}"; do
    [ "$needle" = "$entry" ] && return 0
  done
  return 1
}

if ! command -v testing-conventions >/dev/null 2>&1; then
  echo "error: 'testing-conventions' CLI not found on PATH." >&2
  echo "       install it with: pip install \"testing-conventions==<version>\"" >&2
  echo "       (see .github/workflows/testing-conventions.yml for the pinned version)" >&2
  exit 127
fi

violations=()
for scan in "${SCANS[@]}"; do
  lang="${scan%% *}"
  path="${scan#* }"
  # The CLI exits 1 and prints `missing colocated unit test: <path>` per
  # offender (plus a summary line) to stderr. Capture both streams; the
  # `|| true` stops `set -e` from aborting on that expected non-zero exit.
  out="$(testing-conventions unit location --language "$lang" "$path" 2>&1 || true)"
  while IFS= read -r line; do
    case "$line" in
      "missing colocated unit test: "*)
        file="${line#missing colocated unit test: }"
        is_exempt "$file" || violations+=("$file")
        ;;
    esac
  done <<< "$out"
done

if [ "${#violations[@]}" -gt 0 ]; then
  echo "testing-conventions: source file(s) missing a colocated unit test:" >&2
  printf '  - %s\n' "${violations[@]}" >&2
  echo >&2
  echo "Fix by adding a colocated unit test (foo.py -> foo_test.py, foo.ts -> foo.test.ts)." >&2
  echo "If the file is a genuine entry shim, add it to EXEMPT in scripts/check-testing-conventions.sh with a reason." >&2
  exit 1
fi

echo "testing-conventions: all scanned source files have a colocated unit test (${#EXEMPT[@]} exempt)."
