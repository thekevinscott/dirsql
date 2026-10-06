#!/usr/bin/env bash
# Regenerates (write) or verifies (check) the bash-derived rows of the
# conformance table in docs/reference/glob.md. Needs bash >= 4.3.
set -euo pipefail

mode="${1:-check}"
doc="$(cd "$(dirname "$0")/.." && pwd)/docs/reference/glob.md"

base="$(mktemp -d /tmp/glob-conformance.XXXXXX)"
root="$base/proj"
mkdir "$root"

awk '/<!-- conformance-tree -->/{f=1;next} f&&/^```/{n++;next} f&&n==1{print} n==2{exit}' "$doc" |
while IFS= read -r entry; do
  case "$entry" in
    *' -> '*) ln -s "${entry#* -> }" "$root/${entry%% -> *}" ;;
    */) mkdir -p "$root/$entry" ;;
    *) mkdir -p "$root/$(dirname "$entry")"; printf 'x\n' > "$root/$entry" ;;
  esac
done
printf '*.log\n' > "$root/.gitignore"

expand() {
  local pattern="${1//\$ROOT/$root}"
  pattern="${pattern// /\\ }"
  (cd "$root" && HOME="$root" bash -O globstar -O nullglob -c 'shopt -u dotglob; eval "printf \"%s\\n\" $1"' _ "$pattern") |
    LC_ALL=C sort
}

normalize() {
  sed -e "s#^$root#\$ROOT#" -e 's#^\.\./#$BASE/#' -e 's#^\./##'
}

files_only() {
  local raw="$1" f
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if (cd "$root" && [ -f "$f" ]); then echo "$f"; fi
  done <<<"$raw"
}

out="$(mktemp /tmp/glob-conformance-doc.XXXXXX)"
failed=0
while IFS= read -r line; do
  if [[ "$line" =~ ^\|\ \`([^\`]+)\`\ \|\ (.*)\ \|\ ([a-z.-]*)\ \|$ ]]; then
    pattern="${BASH_REMATCH[1]}"
    cell="${BASH_REMATCH[2]}"
    tag="${BASH_REMATCH[3]}"
    raw="$(expand "$pattern")"
    if [ -n "$tag" ] && [ "$tag" != absolute ]; then
      if [ "$(printf '%s' "$raw" | normalize | sed 's/.*/`&`/' | paste -sd, - | sed 's/,/, /g')" = "$cell" ] || { [ -z "$raw" ] && [ "$cell" = none ]; }; then
        echo "stale divergence tag on $pattern: bash agrees" >&2
        failed=1
      fi
      printf '%s\n' "$line" >>"$out"
      continue
    fi
    kept="$(files_only "$raw")"
    if [[ "$pattern" != *node_modules* ]]; then
      kept="$(grep -v -E '(^|/)node_modules/' <<<"$kept" || true)"
    fi
    kept="$(grep -v -E '\.log$' <<<"$kept" || true)"
    if [ -z "$kept" ]; then
      fresh=none
    else
      fresh="$(printf '%s\n' "$kept" | normalize | LC_ALL=C sort | sed 's/.*/`&`/' | paste -sd, - | sed 's/,/, /g')"
    fi
    if [ "$fresh" != "$cell" ]; then
      [ "$mode" = write ] || { echo "row differs from bash: $pattern" >&2; failed=1; }
    fi
    printf '| `%s` | %s | %s |\n' "$pattern" "$fresh" "$tag" >>"$out"
  else
    printf '%s\n' "$line" >>"$out"
  fi
done <"$doc"

if [ "$mode" = write ]; then
  cp "$out" "$doc"
fi
find "$base" "$out" -delete
exit "$failed"
