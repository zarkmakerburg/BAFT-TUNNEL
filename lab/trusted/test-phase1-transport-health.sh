#!/usr/bin/env bash
# Static acceptance-contract checks for the trusted transport-health LAB harness.
set -Eeuo pipefail

workflow=.github/workflows/lab-transport-health.yml
harness=lab/trusted/phase1-transport-health.sh

bash -n "$harness"
grep -Fq 'Type RUN_LAB_TRANSPORT_HEALTH' "$workflow"
grep -Fq 'environment: baft-tunnel-lab' "$workflow"
grep -Fq 'group: baft-tunnel-trusted-lab-hosts' "$workflow"
grep -Fq 'ref: ${{ github.sha }}' "$workflow"
grep -Fq 'repos/$REPOSITORY/pulls/334' "$workflow"
grep -Fq 'does not contain the merged PR #334 health implementation' "$workflow"
grep -Fq 'sha256sum --check candidate.sha256' "$workflow"
grep -Fq 'TH_REMOTE_BINARY_VERIFIED' "$harness"
grep -Fq 'rtt_ms not a real sample' "$harness"
grep -Fq 'pid does not match the tunnel unit' "$harness"
grep -Fq 'health still reports healthy/authenticated with the carrier cut' "$harness"
grep -Fq 'TH_ACCEPTANCE=PASS rtt_sampled=1' "$harness"
if grep -Fq 'iptables' "$harness" || grep -Fq 'nft ' "$harness" || grep -Fq 'ip route' "$harness"; then
  echo 'the harness must not change firewall or routes' >&2
  exit 1
fi

printf '%s\n' 'TRANSPORT_HEALTH_HARNESS_CONTRACT=PASS manual_confirmation=1 protected_environment=1 serialized_lab=1 exact_main_orchestration=1 artifact_integrity=1 remote_binary_digest=1 real_rtt_sample=1 negative_control=1 no_firewall_changes=1'
