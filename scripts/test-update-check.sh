#!/usr/bin/env bash
# Offline test for update-check.sh (no network): valid stable, refusals, file mode.
set -Eeuo pipefail
S="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/update-check.sh"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
D="$T/state"; mkdir "$D"
ok='{"tag_name":"v0.2.2","prerelease":false,"draft":false}'
echo "$ok" >"$T/ok.json"
bash "$S" --state-dir "$D" --api-json "$T/ok.json" >/dev/null
grep -q '"latest_tag":"v0.2.2"' "$D/update-check.json"
[[ "$(stat -c %a "$D/update-check.json")" == 644 ]]
[[ -z "$(ls -A "$D" | grep -v '^update-check.json$' || true)" ]]
rm "$D/update-check.json"
refuse() { echo "$1" >"$T/bad.json"; if bash "$S" --state-dir "$D" --api-json "$T/bad.json" >/dev/null 2>&1; then echo "FAIL: accepted $1"; exit 1; fi; [[ ! -e "$D/update-check.json" ]]; }
refuse '{"tag_name":"v0.3.0-rc.1","prerelease":true}'
refuse '{"tag_name":"v0.3.0-rc.1","prerelease":false}'
refuse '{"tag_name":"v1.2.3; rm -rf /","prerelease":false}'
refuse '{"tag_name":"latest","prerelease":false}'
if bash "$S" --state-dir "$T/missing" --api-json "$T/ok.json" >/dev/null 2>&1; then echo "FAIL: missing state dir accepted"; exit 1; fi
echo "UPDATE_CHECK=PASS"
