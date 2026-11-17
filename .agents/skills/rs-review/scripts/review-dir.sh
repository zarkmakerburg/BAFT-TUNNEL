#!/usr/bin/env bash
# Print where rs-review reports belong for <start-dir>:  <docs-dir>/rs-reviews
#   review-dir.sh [start-dir]
# Search upward from start-dir to the repository root (or /) for the project's documentation
# folder, preferring internal docs: docs/private → docs/internal → docs. The nearest match wins.
# Only if the project has no docs folder: <workspace root or start-dir>/docs/rs-reviews.
set -uo pipefail
start=$(cd "${1:-.}" && pwd)
top=$(git -C "$start" rev-parse --show-toplevel 2>/dev/null || echo /)
d=$start
while :; do
  for cand in docs/private docs/internal docs; do
    if [[ -d "$d/$cand" ]]; then echo "$d/$cand/rs-reviews"; exit 0; fi
  done
  [[ "$d" == "$top" || "$d" == "/" ]] && break
  d=$(dirname "$d")
done
ws=$(cd "$start" && cargo metadata --no-deps --format-version 1 --offline 2>/dev/null \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["workspace_root"])' 2>/dev/null || echo "$start")
echo "$ws/docs/rs-reviews"
