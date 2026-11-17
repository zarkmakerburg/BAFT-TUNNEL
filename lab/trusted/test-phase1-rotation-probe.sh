#!/usr/bin/env bash
# Static acceptance-contract checks for the trusted session-rotation LAB harness.
set -Eeuo pipefail

workflow=.github/workflows/lab-rotation-probe.yml
harness=lab/trusted/phase1-rotation-probe.sh

bash -n "$harness"
grep -Fq 'Type RUN_LAB_ROTATION_PROBE' "$workflow"
grep -Fq 'environment: baft-tunnel-lab' "$workflow"
grep -Fq 'group: baft-tunnel-trusted-lab-hosts' "$workflow"
grep -Fq 'ref: ${{ github.sha }}' "$workflow"
grep -Fq 'repos/$REPOSITORY/pulls/315' "$workflow"
grep -Fq 'ROTATION_MERGE_SHA' "$workflow"
grep -Fq 'does not contain the merged PR #315 rotation implementation' "$workflow"
grep -Fq 'sha256sum --check candidate.sha256' "$workflow"
grep -Fq 'ROT_REMOTE_BINARY_VERIFIED' "$harness"
grep -Fq 'session rotated: draining the previous session' "$harness"
grep -Fq '(( ROTATION_EVENTS >= 2 ))' "$harness"
grep -Fq '(( OFF_PASS == 0 ))' "$harness"
grep -Fq '(( ON_PASS == PROBES ))' "$harness"
grep -Fq 'ROT_ACCEPTANCE=PASS order=rotation_off_then_rotation_on' "$harness"

printf '%s\n' 'ROTATION_HARNESS_CONTRACT=PASS manual_confirmation=1 protected_environment=1 serialized_lab=1 exact_main_orchestration=1 artifact_integrity=1 remote_binary_digest=1 observable_rotation=1 differential_result_gate=1'
