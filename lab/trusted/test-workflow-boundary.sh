#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
legacy="$root/.github/workflows/phase1-two-host-e2e.yml"
trusted="$root/.github/workflows/phase1-trusted-lab.yml"
# The old entrypoint must not retain any path to LAB credentials or candidate execution.
if grep -Eq 'secrets\.|LAB_SSH_PRIVATE_KEY|phase1-two-host-e2e\.sh' "$legacy"; then
  echo 'Legacy Phase 1 workflow still references secrets or candidate orchestration' >&2
  exit 1
fi
grep -F 'exit 1' "$legacy" >/dev/null
# Only a workflow dispatched from main may reach the secret-bearing environment.
grep -F "if: \${{ github.ref == 'refs/heads/main' }}" "$trusted" >/dev/null
grep -F 'ref: audit/pr150-remediation-2026-11-10' "$trusted" >/dev/null
grep -F '[[ "$actual" == "$REQUESTED_SHA" ]]' "$trusted" >/dev/null
grep -F 'ref: ${{ github.sha }}' "$trusted" >/dev/null
grep -F 'run: bash lab/trusted/phase1-direct-smoke.sh' "$trusted" >/dev/null
# Candidate validation/build jobs must not receive secrets.
if awk '/^  build-candidate:/{inside=1} /^  two-host-lab:/{inside=0} inside && /secrets\./{found=1} END{exit !found}' "$trusted"; then
  echo 'Candidate build job has access to secrets' >&2
  exit 1
fi
# Exactly the four LAB credentials are declared, and only in the trusted LAB job.
test "$(grep -c 'secrets\.' "$trusted")" = 4
# Candidate processes are privilege-dropped; the SSH private key stays in the runner harness.
grep -F 'runuser -u nobody' "$root/lab/trusted/phase1-direct-smoke.sh" >/dev/null
grep -F 'ssh_key_exposed_to_candidate=0' "$root/lab/trusted/phase1-direct-smoke.sh" >/dev/null
echo 'Trusted Phase 1 workflow boundary tests: PASS'
