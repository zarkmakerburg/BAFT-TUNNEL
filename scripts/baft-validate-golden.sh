#!/usr/bin/env bash
# Run the pinned BAFT `config validate` on the golden configs rendered by src/baft_engine.rs.
# Usage: scripts/baft-validate-golden.sh /path/to/baft
# Proves only that BAFT accepts the rendered configuration shape. It starts nothing.
set -euo pipefail
baft="${1:?usage: $0 /path/to/baft}"
here="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
for name in serve forward; do
  sed "s#/run/baft-tunnel/t1#$work#g" "$here/src/testdata/baft/$name.json" > "$work/$name.json"
done
: > "$work/revocation.json"
for name in serve forward; do
  "$baft" config validate --file "$work/$name.json"
done
# Negative control: a config BAFT must refuse (plaintext payload logging).
sed 's#"payload": false#"payload": true#' "$work/serve.json" > "$work/bad.json"
if "$baft" config validate --file "$work/bad.json" >/dev/null 2>&1; then
  echo "BAFT accepted a config with payload logging enabled" >&2
  exit 1
fi
echo "BAFT_GOLDEN_VALIDATE=PASS"
