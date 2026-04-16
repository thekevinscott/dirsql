#!/usr/bin/env bash
# SessionStart hook: link agents/build/environment.md to the appropriate
# environment file based on the Claude Code CLAUDE_CODE_REMOTE env var.
#
# - CLAUDE_CODE_REMOTE=true  -> agents/environments/remote.md
# - anything else            -> agents/environments/local.md
#
# Runs on every session start (startup + resume). Output goes to stderr so it
# is surfaced in the transcript but not injected as additional context.

set -euo pipefail

project_dir="${CLAUDE_PROJECT_DIR:-$PWD}"
build_dir="${project_dir}/agents/build"
link_path="${build_dir}/environment.md"

if [ "${CLAUDE_CODE_REMOTE:-}" = "true" ]; then
  target="../environments/remote.md"
else
  target="../environments/local.md"
fi

mkdir -p "${build_dir}"
ln -sfn "${target}" "${link_path}"

echo "[select-environment] linked ${link_path} -> ${target}" >&2
