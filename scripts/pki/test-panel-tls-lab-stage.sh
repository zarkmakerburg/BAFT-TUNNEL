#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
STAGE=lab/real-hosts/panel-pairing-stage.sh
MAIN=lab/real-hosts/phase1-two-host-e2e.sh
for route in '/v1/pki/csr/create' '/v1/pki/csr/read' '/v1/pki/certificate/install'; do
  grep -Fq "$route" "$STAGE" || { echo "missing D2 Agent API route in two-host stage: $route" >&2; exit 1; }
done
grep -Fq -- '--network none' "$STAGE" || { echo "offline CA container must have no network" >&2; exit 1; }
grep -Fq 'service_restarted' "$STAGE" || { echo "LAB must assert certificate import does not restart the service" >&2; exit 1; }
grep -Fq 'private_key_exported' "$STAGE" || { echo "LAB must assert the private key never leaves the node" >&2; exit 1; }
grep -Fq 'pki_files' "$STAGE"
grep -Fq 'tcp-tls' "$MAIN" || { echo "two-host runner does not expose the tcp-tls panel stage" >&2; exit 1; }
grep -Fq 'LAB_PANEL_ENGINE' "$MAIN" || { echo "two-host runner does not bind the panel engine input" >&2; exit 1; }
bash -n "$STAGE" "$MAIN"
echo "PANEL_TLS_LAB_STAGE_STATIC_TEST=PASS"
