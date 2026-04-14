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

# Accept either APPROVED_* (explicit) or ROBOT_* (set by the cc/cx wrappers
# in ~/work/dotfiles). The wrapper convention is documented in AGENTS.md.
: "${APPROVED_GIT_NAME:=${ROBOT_GIT_NAME-}}"
: "${APPROVED_GIT_EMAIL:=${ROBOT_GIT_EMAIL-}}"
: "${APPROVED_GPG_KEY:=${ROBOT_GPG_KEY_ID-}}"
: "${AGENT_NAME:=${ROBOT_AGENT_NAME-}}"
: "${AGENT_MODEL:=${ROBOT_AGENT_MODEL-}}"

require_env APPROVED_GIT_NAME
require_env APPROVED_GIT_EMAIL
require_env AGENT_NAME
require_env AGENT_MODEL
require_env APPROVED_GPG_KEY

signing_key="$(git config --get user.signingkey || true)"
commit_gpgsign="$(git config --bool --get commit.gpgsign || true)"
author_ident="$(git var GIT_AUTHOR_IDENT)"
committer_ident="$(git var GIT_COMMITTER_IDENT)"
effective_author_name="$(printf '%s\n' "$author_ident" | sed -E 's/^(.*) <.*$/\1/')"
effective_author_email="$(printf '%s\n' "$author_ident" | sed -E 's/^.*<(.*)>.*$/\1/')"
effective_committer_name="$(printf '%s\n' "$committer_ident" | sed -E 's/^(.*) <.*$/\1/')"
effective_committer_email="$(printf '%s\n' "$committer_ident" | sed -E 's/^.*<(.*)>.*$/\1/')"

[ "$effective_author_name" = "$APPROVED_GIT_NAME" ] || fail "effective author name does not match APPROVED_GIT_NAME"
[ "$effective_author_email" = "$APPROVED_GIT_EMAIL" ] || fail "effective author email does not match APPROVED_GIT_EMAIL"
[ "$effective_committer_name" = "$APPROVED_GIT_NAME" ] || fail "effective committer name does not match APPROVED_GIT_NAME"
[ "$effective_committer_email" = "$APPROVED_GIT_EMAIL" ] || fail "effective committer email does not match APPROVED_GIT_EMAIL"
[ "$signing_key" = "$APPROVED_GPG_KEY" ] || fail "git user.signingkey does not match APPROVED_GPG_KEY"
[ "$commit_gpgsign" = "true" ] || fail "git commit.gpgsign must be true"

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
        git verify-commit HEAD >/dev/null 2>&1 || fail "HEAD commit is not signed or the signature cannot be verified locally"

        if [ "$action" = "pr" ]; then
            require_env GH_TOKEN
        fi

        echo "agent-preflight: ok for $action using approved robot identity $APPROVED_GIT_EMAIL"
        ;;
esac
