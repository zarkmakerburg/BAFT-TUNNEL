#!/usr/bin/env bash
set -Eeuo pipefail
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
HELPER="$ROOT/lab/real-hosts/clean-test-host.sh"
WORKFLOW="$ROOT/.github/workflows/phase1-trusted-pairing-lab.yml"
bash -n "$HELPER"
grep -Fq "CLEAN_BAFT_TUNNEL_TEST_HOST" "$HELPER"
grep -Fq "VERSION_ID:-" "$HELPER"
grep -Fq "/run/baft-tunnel" "$HELPER"
grep -Fq "/etc/baft-tunnel" "$HELPER"
grep -Fq "/var/lib/baft-tunnel-agent" "$HELPER"
grep -Fq "baft-tunnel-install.*" "$HELPER"
if grep -Eq "rm -rf --? /([[:space:]]|$)|iptables|nft |sysctl|reboot|ufw" "$HELPER"; then
  echo "cleanup helper exceeds BAFT-TUNNEL artifact scope" >&2
  exit 1
fi
grep -Fq "clean_test_hosts:" "$WORKFLOW"
grep -Fq "CLEAN_BAFT_TUNNEL_TEST_HOSTS" "$WORKFLOW"
grep -Fq "for side in iran abroad" "$WORKFLOW"
grep -Fq "bash -s -- CLEAN_BAFT_TUNNEL_TEST_HOST" "$WORKFLOW"
echo "Test-host cleanup scope and workflow gate: PASS"
