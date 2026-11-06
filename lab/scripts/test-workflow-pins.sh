#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/good" "$tmp/bad"
cat > "$tmp/good/workflow.yml" <<'GOOD'
steps:
  - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # pinned
  - uses: ./local-action
GOOD
cat > "$tmp/bad/workflow.yml" <<'BAD'
steps:
  - uses: actions/checkout@v7
BAD
sh "$root/lab/scripts/check-workflow-pins.sh" "$tmp/good" >/dev/null
if sh "$root/lab/scripts/check-workflow-pins.sh" "$tmp/bad" >/dev/null 2>&1; then
  echo 'mutable action tag unexpectedly accepted' >&2
  exit 1
fi
echo 'Workflow action pin policy tests: PASS (immutable SHA accepted; mutable tag rejected)'
