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
grep -F 'ref: ${{ github.sha }}' "$trusted" >/dev/null
grep -F 'git merge-base --is-ancestor "$actual" "$GITHUB_SHA"' "$trusted" >/dev/null
grep -F '[[ "$actual" == "$REQUESTED_SHA" ]]' "$trusted" >/dev/null
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
# The D2 workflow also checks out trusted main orchestration; only the candidate binary is built separately.
d2="$root/.github/workflows/phase1-trusted-d2-lab.yml"
grep -F "if: \${{ github.ref == 'refs/heads/main' }}" "$d2" >/dev/null
grep -F 'ref: ${{ github.sha }}' "$d2" >/dev/null
grep -F 'git merge-base --is-ancestor "$actual" "$GITHUB_SHA"' "$d2" >/dev/null
grep -F 'git diff --name-only "$actual" "$GITHUB_SHA"' "$d2" >/dev/null
grep -F 'lab/real-hosts/panel-pairing-stage\.sh' "$d2" >/dev/null
grep -F 'scripts/pki/test-panel-tls-lab-stage\.sh' "$d2" >/dev/null
grep -F 'bash lab/real-hosts/phase1-two-host-e2e.sh' "$d2" >/dev/null
grep -F 'LAB_PANEL_PAIRING=1 LAB_PANEL_ENGINE=tcp-tls' "$d2" >/dev/null
grep -F 'unset LAB_SSH_PRIVATE_KEY LAB_SSH_KNOWN_HOSTS' "$d2" >/dev/null
grep -F 'environment: baft-tunnel-lab' "$d2" >/dev/null
if awk '/^  build-candidate:/{inside=1} /^  d2-two-host-lab:/{inside=0} inside && /secrets\./{found=1} END{exit !found}' "$d2"; then
  echo 'D2 candidate build job has access to LAB secrets' >&2
  exit 1
fi
test "$(grep -c 'secrets\.' "$d2")" = 4
echo 'Trusted Phase 1 workflow boundary tests: PASS'
