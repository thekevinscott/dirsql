#!/bin/sh

set -eu

usage() {
    echo "Usage: scripts/agent-preflight.sh <commit|push|pr>" >&2
    exit 2
}

fail() {
    echo "agent-preflight: $1" >&2
    exit 1
}

[ $# -eq 1 ] || usage

action="$1"
case "$action" in
    commit|push|pr) ;;
    *) usage ;;
esac

git rev-parse --show-toplevel >/dev/null 2>&1 || fail "not inside a git repository"

require_env() {
    var_name="$1"
    eval "value=\${$var_name-}"
    [ -n "$value" ] || fail "missing required environment variable: $var_name"
}

require_env APPROVED_GIT_NAME
require_env APPROVED_GIT_EMAIL
require_env AGENT_NAME
require_env AGENT_MODEL

configured_name="$(git config --get user.name || true)"
configured_email="$(git config --get user.email || true)"
use_config_only="$(git config --bool --get user.useConfigOnly || true)"

[ "$configured_name" = "$APPROVED_GIT_NAME" ] || fail "git user.name does not match APPROVED_GIT_NAME"
[ "$configured_email" = "$APPROVED_GIT_EMAIL" ] || fail "git user.email does not match APPROVED_GIT_EMAIL"
[ "$use_config_only" = "true" ] || fail "git user.useConfigOnly must be true"

trailer="Agent: $AGENT_NAME ($AGENT_MODEL)"

case "$action" in
    commit)
        echo "agent-preflight: ok for commit using approved robot identity $APPROVED_GIT_EMAIL"
        echo "agent-preflight: include this trailer in the commit message:"
        echo "$trailer"
        ;;
    push|pr)
        head_author_email="$(git log -1 --format=%ae HEAD)"
        head_committer_email="$(git log -1 --format=%ce HEAD)"
        head_message="$(git log -1 --format=%B HEAD)"

        [ "$head_author_email" = "$APPROVED_GIT_EMAIL" ] || fail "HEAD author email does not match APPROVED_GIT_EMAIL"
        [ "$head_committer_email" = "$APPROVED_GIT_EMAIL" ] || fail "HEAD committer email does not match APPROVED_GIT_EMAIL"
        printf '%s\n' "$head_message" | grep -Fqx "$trailer" || fail "HEAD commit is missing trailer: $trailer"

        if [ "$action" = "pr" ]; then
            require_env GH_TOKEN
        fi

        echo "agent-preflight: ok for $action using approved robot identity $APPROVED_GIT_EMAIL"
        ;;
esac
