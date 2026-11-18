#!/usr/bin/env bash
# Static acceptance-contract checks for the trusted stream-resume chaos LAB harness.
set -Eeuo pipefail

workflow=.github/workflows/lab-resume-chaos.yml
harness=lab/trusted/phase1-resume-chaos.sh

bash -n "$harness"
grep -Fq 'Type RUN_LAB_RESUME_CHAOS' "$workflow"
grep -Fq 'environment: baft-tunnel-lab' "$workflow"
grep -Fq 'group: baft-tunnel-trusted-lab-hosts' "$workflow"
grep -Fq 'ref: ${{ github.sha }}' "$workflow"
grep -Fq 'repos/$REPOSITORY/pulls/365' "$workflow"
grep -Fq 'does not contain the merged PR #365 resume implementation' "$workflow"
grep -Fq 'sha256sum --check candidate.sha256' "$workflow"
grep -Fq 'RES_REMOTE_BINARY_VERIFIED' "$harness"
grep -Fq '"protocol_version": 3' "$harness"
grep -Fq '(( RELAY_CUTS >= 2 ))' "$harness"
grep -Fq '(( RESUME_EVENTS >= 2 ))' "$harness"
grep -Fq 'a tunnel service restarted during the chaos run' "$harness"
grep -Fq 'RES_ACCEPTANCE=PASS sha256_equal=1' "$harness"
if grep -Fq 'iptables' "$harness" || grep -Fq 'nft ' "$harness" || grep -Fq 'ip route' "$harness"; then
  echo 'the harness must not change firewall or routes' >&2
  exit 1
fi

printf '%s\n' 'RESUME_CHAOS_HARNESS_CONTRACT=PASS manual_confirmation=1 protected_environment=1 serialized_lab=1 exact_main_orchestration=1 artifact_integrity=1 remote_binary_digest=1 observable_cuts=1 observable_resume=1 no_restarts=1 no_firewall_changes=1'
