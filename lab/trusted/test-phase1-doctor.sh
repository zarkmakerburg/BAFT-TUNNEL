#!/usr/bin/env bash
# Static acceptance-contract checks for the trusted Doctor LAB harness.
set -Eeuo pipefail

workflow=.github/workflows/lab-doctor.yml
harness=lab/trusted/phase1-doctor.sh

bash -n "$harness"
grep -Fq 'Type RUN_LAB_DOCTOR' "$workflow"
grep -Fq 'environment: baft-tunnel-lab' "$workflow"
grep -Fq 'group: baft-tunnel-trusted-lab-hosts' "$workflow"
grep -Fq 'ref: ${{ github.sha }}' "$workflow"
grep -Fq 'repos/$REPOSITORY/pulls/298' "$workflow"
grep -Fq 'does not contain the merged PR #298 Doctor implementation' "$workflow"
grep -Fq 'sha256sum --check candidate.sha256' "$workflow"
grep -Fq 'DC_REMOTE_BINARY_VERIFIED' "$harness"
grep -Fq 'network reach check not PASS with the abroad tunnel listening' "$harness"
grep -Fq 'doctor did not FAIL the reach check with the abroad tunnel stopped' "$harness"
grep -Fq 'DC_ACCEPTANCE=PASS config_checks=1' "$harness"
if grep -Fq 'iptables' "$harness" || grep -Fq 'nft ' "$harness" || grep -Fq 'ip route' "$harness"; then
  echo 'the harness must not change firewall or routes' >&2
  exit 1
fi
if grep -Fq 'echo "$LAB_SSH' "$harness" || grep -Eq -- '--insecure|curl -k' "$harness"; then
  echo 'the harness must not print credentials or disable certificate verification' >&2
  exit 1
fi

printf '%s\n' 'DOCTOR_HARNESS_CONTRACT=PASS manual_confirmation=1 protected_environment=1 serialized_lab=1 exact_main_orchestration=1 artifact_integrity=1 remote_binary_digest=1 reach_up_down=1 read_only=1 no_firewall_changes=1'
