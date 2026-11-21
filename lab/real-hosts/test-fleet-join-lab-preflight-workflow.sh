#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
WORKFLOW="$ROOT/.github/workflows/fleet-join-lab-preflight.yml"
OLD_WORKFLOW="$ROOT/.github/workflows/phase0-two-host-lab.yml"
fail() { echo "FLEET_JOIN_LAB_PREFLIGHT_WORKFLOW_TEST=FAIL: $*" >&2; exit 1; }
need() { grep -Fq -- "$2" "$1" || fail "missing required contract: $2"; }

[[ -f "$WORKFLOW" ]] || fail "dedicated workflow is missing"
need "$WORKFLOW" 'workflow_dispatch:'
need "$WORKFLOW" 'FLEET_JOIN_READ_ONLY_PREFLIGHT'
need "$WORKFLOW" 'refs/heads/main'
need "$WORKFLOW" 'permissions:'
need "$WORKFLOW" 'contents: read'
need "$WORKFLOW" 'environment: baft-tunnel-fleet-lab-20261121'
need "$WORKFLOW" 'group: baft-tunnel-phase0-two-host-lab'
need "$WORKFLOW" 'secrets.LAB_IRAN_HOST'
need "$WORKFLOW" 'secrets.LAB_ABROAD_HOST'
need "$WORKFLOW" 'secrets.LAB_SSH_PRIVATE_KEY'
need "$WORKFLOW" 'secrets.LAB_SSH_KNOWN_HOSTS'
need "$WORKFLOW" 'vars.LAB_IRAN_SSH_USER'
need "$WORKFLOW" 'vars.LAB_ABROAD_SSH_USER'
need "$WORKFLOW" 'vars.LAB_IRAN_SSH_PORT'
need "$WORKFLOW" 'vars.LAB_ABROAD_SSH_PORT'
need "$WORKFLOW" 'vars.LAB_TEST_PORTS'
need "$WORKFLOW" 'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1'
need "$WORKFLOW" 'persist-credentials: false'
need "$WORKFLOW" 'mktemp -d'
need "$WORKFLOW" 'trap cleanup EXIT'
need "$ROOT/lab/real-hosts/preflight-two-hosts.sh" 'StrictHostKeyChecking=yes'
need "$WORKFLOW" 'bash lab/real-hosts/preflight-two-hosts.sh'
need "$WORKFLOW" 'rm -rf --'
need "$OLD_WORKFLOW" 'group: baft-tunnel-phase0-two-host-lab'

if grep -Eq '^[[:space:]]+(schedule|workflow_call):' "$WORKFLOW"; then
  fail "workflow must be manual-only"
fi
if grep -Eq 'sudo |systemctl (start|stop|restart|enable|disable)|reboot|nft (add|delete)|ip route (add|del)' "$WORKFLOW"; then
  fail "workflow contains a state-changing host operation"
fi

echo 'FLEET_JOIN_LAB_PREFLIGHT_WORKFLOW_TEST=PASS (manual confirmation, main-only, dedicated environment, shared serialization, pinned actions/SSH, ephemeral credential cleanup, read-only script)'
