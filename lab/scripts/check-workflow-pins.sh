#!/bin/sh
set -eu
workflow_dir=${1:-.github/workflows}
found=0
for workflow in "$workflow_dir"/*.yml "$workflow_dir"/*.yaml; do
  [ -f "$workflow" ] || continue
  while IFS= read -r line; do
    value=$(printf '%s\n' "$line" | sed -nE 's/^[[:space:]]*uses:[[:space:]]*([^[:space:]#]+).*$/\1/p')
    [ -n "$value" ] || continue
    case "$value" in
      ./*|docker://*) continue ;;
      *@*)
        ref=${value##*@}
        if [ "${#ref}" -ne 40 ] || ! printf '%s\n' "$ref" | grep -Eq '^[0-9a-fA-F]{40}$'; then
          echo "Unpinned GitHub Action reference in $workflow: $value" >&2
          exit 1
        fi
        ;;
      *)
        echo "GitHub Action reference has no immutable SHA in $workflow: $value" >&2
        exit 1
        ;;
    esac
    found=1
  done < "$workflow"
done
if [ "$found" -eq 0 ]; then
  echo "No external GitHub Actions found in $workflow_dir" >&2
  exit 1
fi
echo "Workflow action pin check: PASS (all external actions use full 40-character commit SHAs)"
