#!/usr/bin/env bash
# Static acceptance-contract checks for the trusted stream-multi-link failover LAB harness.
set -Eeuo pipefail

workflow=.github/workflows/lab-multilink-failover.yml
harness=lab/trusted/phase1-multilink-failover.sh

bash -n "$harness"
grep -Fq 'Type RUN_LAB_MULTILINK_FAILOVER' "$workflow"
grep -Fq 'environment: baft-tunnel-lab' "$workflow"
grep -Fq 'group: baft-tunnel-trusted-lab-hosts' "$workflow"
grep -Fq 'ref: ${{ github.sha }}' "$workflow"
grep -Fq 'repos/$REPOSITORY/pulls/296' "$workflow"
grep -Fq 'does not contain the merged PR #296 failover implementation' "$workflow"
grep -Fq 'sha256sum --check candidate.sha256' "$workflow"
grep -Fq 'MLF_REMOTE_BINARY_VERIFIED' "$harness"
grep -Fq 'backup_remotes=' "$harness"
grep -Fq '(( SWITCHES >= 1 ))' "$harness"
grep -Fq 'a tunnel service restarted during the failover' "$harness"
grep -Fq 'MLF_ACCEPTANCE=PASS failover_to_backup=1' "$harness"
if grep -Fq 'iptables' "$harness" || grep -Fq 'nft ' "$harness" || grep -Fq 'ip route' "$harness"; then
  echo 'the harness must not change firewall or routes' >&2
  exit 1
fi

printf '%s\n' 'MULTILINK_FAILOVER_HARNESS_CONTRACT=PASS manual_confirmation=1 protected_environment=1 serialized_lab=1 exact_main_orchestration=1 artifact_integrity=1 remote_binary_digest=1 observable_switch=1 verified_payload_after_failure=1 no_restarts=1 no_firewall_changes=1'
