#!/usr/bin/env bash
# Static acceptance-contract checks for the trusted Fleet report LAB harness.
set -Eeuo pipefail

workflow=.github/workflows/lab-fleet-report.yml
harness=lab/trusted/phase1-fleet-report.sh

bash -n "$harness"
grep -Fq 'Type RUN_LAB_FLEET_REPORT' "$workflow"
grep -Fq 'environment: baft-tunnel-lab' "$workflow"
grep -Fq 'group: baft-tunnel-trusted-lab-hosts' "$workflow"
grep -Fq 'ref: ${{ github.sha }}' "$workflow"
grep -Fq 'repos/$REPOSITORY/pulls/383' "$workflow"
grep -Fq 'does not contain the merged PR #383 Fleet wiring implementation' "$workflow"
grep -Fq 'sha256sum --check candidate.sha256' "$workflow"
grep -Fq 'FL_REMOTE_BINARY_VERIFIED' "$harness"
grep -Fq 'FL_NODE_LIVE' "$harness"
grep -Fq 'a tunnel-profile client certificate was not refused with 403' "$harness"
grep -Fq 'a client without a certificate got an HTTP answer' "$harness"
grep -Fq 'the node is still live at the hub after the member stopped' "$harness"
grep -Fq 'FL_ACCEPTANCE=PASS member_live=1' "$harness"
if grep -Fq 'iptables' "$harness" || grep -Fq 'nft ' "$harness" || grep -Fq 'ip route' "$harness"; then
  echo 'the harness must not change firewall or routes' >&2
  exit 1
fi
if grep -Fq 'echo "$LAB_SSH' "$harness" || grep -Eq 'Bearer \$\(?token|--insecure|curl -k|-k ' "$harness"; then
  echo 'the harness must not print credentials or disable certificate verification' >&2
  exit 1
fi

printf '%s\n' 'FLEET_REPORT_HARNESS_CONTRACT=PASS manual_confirmation=1 protected_environment=1 serialized_lab=1 exact_main_orchestration=1 artifact_integrity=1 remote_binary_digest=1 member_live=1 negative_controls=1 stale_after_stop=1 no_firewall_changes=1 verified_tls=1'
