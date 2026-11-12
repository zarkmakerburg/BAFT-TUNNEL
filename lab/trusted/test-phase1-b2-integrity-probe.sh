#!/usr/bin/env bash
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
workflow="$root/.github/workflows/phase1-trusted-b2-lab.yml"
harness="$root/lab/trusted/phase1-b2-integrity-probe.sh"
test -s "$workflow"
test -x "$harness"
bash -n "$harness"
grep -F 'install -m 600 /dev/null "$SSH_KEY"' "$harness" >/dev/null
grep -F 'chmod 600 "$KNOWN_HOSTS"' "$harness" >/dev/null
grep -F 'unset LAB_SSH_PRIVATE_KEY LAB_SSH_KNOWN_HOSTS' "$harness" >/dev/null
grep -F 'GlobalKnownHostsFile=/dev/null' "$harness" >/dev/null
grep -F 'StrictHostKeyChecking=yes' "$harness" >/dev/null
grep -F 'runuser -u baft-tunnel --' "$harness" >/dev/null
grep -F 'REMOTE_BASE="/var/tmp/baft-b2-lab-$RUN_ID"' "$harness" >/dev/null
grep -F 'command -v findmnt >/dev/null' "$harness" >/dev/null
grep -F 'mount_options=\$(findmnt -n -o OPTIONS --target /var/tmp)' "$harness" >/dev/null
grep -F 'case ",\$mount_options," in *,noexec,*' "$harness" >/dev/null
grep -F 'stat.S_ISDIR(s.st_mode)' "$harness" >/dev/null
python3 - "$harness" <<'PYQUOTE'
from pathlib import Path
import sys
line = next(line for line in Path(sys.argv[1]).read_text().splitlines() if "python3 -c 'import os,stat;" in line)
assert 'os.stat(\\"/var/tmp\\"' in line, line
PYQUOTE
grep -F 'mkdir -m 0755' "$harness" >/dev/null
grep -F '&& test \$(stat -c' "$harness" >/dev/null
grep -F 'runuser -u baft-tunnel-agent --' "$harness" >/dev/null
grep -F -- '--property=User=baft-tunnel ' "$harness" >/dev/null
grep -F -- '--property=User=baft-tunnel-agent ' "$harness" >/dev/null
grep -F -- '--property=SupplementaryGroups=baft-tunnel' "$harness" >/dev/null
grep -F -- '--property=RuntimeDirectoryMode=0750' "$harness" >/dev/null
test "$(grep -F -o 'RuntimeDirectory=baft-tunnel/$INSTANCE' "$harness" | wc -l | tr -d ' ')" = 2
test "$(grep -o -- '--property=RuntimeDirectoryMode=0750' "$harness" | wc -l | tr -d ' ')" = 2
grep -F 'ABROAD_TUNNEL_UNIT="$(tunnel_unit_for abroad)"' "$harness" >/dev/null
grep -F 'IRAN_TUNNEL_UNIT="$(tunnel_unit_for iran)"' "$harness" >/dev/null
if grep -E 'systemd-run.*agent_unit.*RuntimeDirectory=' "$harness" >/dev/null; then
  echo 'Agent must not own the shared per-instance RuntimeDirectory' >&2; exit 1
fi

grep -F 'InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' "$harness" >/dev/null
grep -F 'existing BAFT-TUNNEL runtime path found' "$harness" >/dev/null
grep -F 'existing BAFT-TUNNEL privileged control helper found' "$harness" >/dev/null
grep -F 'parent_mode=' "$harness" >/dev/null
grep -F 'root:baft-tunnel:770' "$harness" >/dev/null
grep -F 'SystemCallFilter=@system-service' "$harness" >/dev/null
grep -F 'CapabilityBoundingSet=' "$harness" >/dev/null
grep -F 'MemoryDenyWriteExecute=yes' "$harness" >/dev/null
test "$(grep -o 'IPAddressDeny=any' "$harness" | wc -l | tr -d ' ')" -ge 4
grep -F 'IPAddressAllow=127.0.0.0/8 ::1/128 $IRAN_CIDR' "$harness" >/dev/null
grep -F 'IPAddressAllow=127.0.0.0/8 ::1/128 $ABROAD_CIDR' "$harness" >/dev/null
grep -F 'IPAddressAllow=127.0.0.0/8 ::1/128' "$harness" >/dev/null
grep -F 'NETWORK_EGRESS_POLICY_READY' "$harness" >/dev/null
grep -F 'denied in ({"any"},{"0.0.0.0/0","::/0"})' "$harness" >/dev/null
test "$(grep -c 'InaccessiblePaths=-/etc/baft-tunnel -/var/lib/baft-tunnel -/var/lib/baft-tunnel-agent -/run/baft-tunnel/control' "$harness")" = 3
if grep -Eq 'bash .*scripts/|bash .*lab/|sudo |install\.sh|quick-install' "$harness"; then
  echo 'B2 harness attempted to execute candidate-controlled scripts or installers' >&2; exit 1
fi
if grep -Eqi 'iptables|nft |ufw |firewall-cmd|sysctl -w|useradd|groupadd|apt-get|dnf install' "$harness"; then
  echo 'B2 harness contains host policy/account/package mutations' >&2; exit 1
fi
test "$(grep -c -- '--protocol-version 2' "$harness")" = 2
grep -F 'setup_v2_selection=1' "$harness" >/dev/null
if grep -E 'set_protocol_v2|setup_config_override|PROTOCOL_V2_CONFIG_READY|config validate --config|config plan --config' "$harness" >/dev/null; then
  echo 'B2 LAB must exercise protocol-v2 selection through normal setup, not mutate generated configs' >&2; exit 1
fi
grep -F '^[0-9a-f]{8}(-[0-9a-f]{8}){3}$' "$harness" >/dev/null
test "$(grep -c '^verify_probe ' "$harness")" = 2
grep -F 'CONTROL_SOCKET_READY' "$harness" >/dev/null
grep -F 'id -u baft-tunnel-agent' "$harness" >/dev/null
grep -F 'id -gn baft-tunnel-agent' "$harness" >/dev/null
grep -F 'PROBE_COOLDOWN_PASS' "$harness" >/dev/null
grep -F 'LAST_PROBE_PERSISTED_AFTER_AGENT_RESTART_PASS' "$harness" >/dev/null
grep -F 'LAST_PROBE_COOLDOWN_PERSISTED_PASS' "$harness" >/dev/null
grep -F 'INTEGRITY_AUDIT_EVENTS_PASS' "$harness" >/dev/null
grep -F 'integrity.get("result") == "unknown"' "$harness" >/dev/null
grep -F 'systemctl restart' "$harness" >/dev/null
if grep -E "systemd-run --quiet --collect --unit=.*agent_unit" "$harness" >/dev/null; then
  echo 'Agent transient unit must remain restartable for the last-probe persistence check' >&2; exit 1
fi

grep -F 'POST /v1/verify ""' "$harness" >/dev/null
if grep -E 'idempotency_key|replayed|IDEMPOTENCY' "$harness" >/dev/null; then
  echo 'current PR #177 verify API has no idempotency request body' >&2; exit 1
fi
grep -F 'INTEGRITY_PROBE_DID_NOT_TOUCH_TCP_TARGET=PASS' "$harness" >/dev/null
grep -F 'DATA_PATH_OK' "$harness" >/dev/null
grep -F 'wait_listener abroad "$ECHO_PORT"' "$harness" >/dev/null
grep -F 'test "$(uname -m)" = x86_64' "$harness" >/dev/null
grep -F 'RuntimeDirectory=baft-tunnel/$INSTANCE' "$harness" >/dev/null
grep -F 'peer_id' "$harness" >/dev/null
grep -F 'direction' "$harness" >/dev/null
grep -F 'LAB cleanup incomplete' "$harness" >/dev/null
grep -F 'systemctl is-active --quiet' "$harness" >/dev/null
grep -F "rm -rf '\$REMOTE_BASE'" "$harness" >/dev/null
grep -F 'github.ref == ' "$workflow" >/dev/null
grep -F 'gh api "repos/$REPOSITORY/pulls/177"' "$workflow" >/dev/null
grep -F 'commits/$REQUESTED_SHA/check-runs' "$workflow" >/dev/null
grep -F 'required_checks=(rust ubuntu-2204-baseline lab-config)' "$workflow" >/dev/null
grep -F '.state == "closed"' "$workflow" >/dev/null
grep -F '.merged == true' "$workflow" >/dev/null
grep -F '.head.sha == $sha' "$workflow" >/dev/null
grep -F 'git merge-base --is-ancestor "$REQUESTED_SHA" "$merge_sha"' "$workflow" >/dev/null
grep -F 'git merge-base --is-ancestor "$merge_sha" "$GITHUB_SHA"' "$workflow" >/dev/null
grep -F 'environment: baft-tunnel-lab' "$workflow" >/dev/null
grep -F 'checks: read' "$workflow" >/dev/null
grep -F 'needs.validate-candidate.outputs.candidate_sha' "$workflow" >/dev/null
grep -F 'github.sha' "$workflow" >/dev/null
grep -F 'run: bash lab/trusted/phase1-b2-integrity-probe.sh' "$workflow" >/dev/null
if awk '/^  build-candidate:/{inside=1} /^  two-host-b2-lab:/{inside=0} inside && /secrets\./{found=1} END{exit !found}' "$workflow"; then
  echo 'B2 candidate build job has access to LAB secrets' >&2; exit 1
fi
test "$(grep -c 'secrets\.' "$workflow")" = 4
echo 'Trusted B2 LAB workflow boundary tests: PASS (current PR #177 API contract; static checks only; no hosts contacted)'
